use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use super::test_support::*;
use super::*;
use crate::ssh_profiles::SshAuthMode;
use crate::ssh_session::route::{ResolvedRoute, RouteTransport};

#[test]
fn automatic_authentication_always_contains_the_agent_step() {
    assert_eq!(
        crate::ssh_session::authentication_plan(SshAuthMode::Auto, &[], &[]),
        vec![
            crate::ssh_session::AuthMethod::Agent,
            crate::ssh_session::AuthMethod::StoredPassword,
            crate::ssh_session::AuthMethod::KeyboardInteractive,
            crate::ssh_session::AuthMethod::PromptPassword,
        ]
    );
}

#[test]
fn agent_only_keys_authenticate_formal_and_test_connections_including_legacy_mode() {
    check(async {
        let key = key();
        let agent = Agent::new(vec![Identity::plain(key.clone())]);
        let mut fixture = Fixture::new(ServerOptions {
            key: Some(key.public_key().clone()),
            ..Default::default()
        })
        .await;
        fixture.route.profile.auth = serde_json::from_str("\"agent\"").unwrap();
        assert_eq!(fixture.route.profile.auth, SshAuthMode::Auto);
        with_factory(move |_, _| Ok(agent.connect()), async {
            fixture.formal().await.unwrap();
            fixture.test(Some("unused draft")).await.unwrap();
        })
        .await;
        assert_eq!(fixture.stats.accepted.load(Ordering::SeqCst), 2);
        assert_eq!(fixture.stats.passwords.load(Ordering::SeqCst), 0);
    });
}

#[test]
fn certificates_are_signed_by_the_agent_and_accepted_by_the_real_server() {
    check(async {
        let key = key();
        let agent = Agent::new(vec![Identity::certificate(key.clone())]);
        let fixture = Fixture::new(ServerOptions {
            key: Some(key.public_key().clone()),
            ..Default::default()
        })
        .await;
        with_factory(move |_, _| Ok(agent.connect()), async {
            fixture.formal().await.unwrap();
            fixture.test(None).await.unwrap();
        })
        .await;
        assert_eq!(fixture.stats.certificates.load(Ordering::SeqCst), 2);
    });
}

#[test]
fn rsa_agent_signatures_follow_the_servers_sha256_or_sha512_advertisement() {
    use russh::keys::ssh_key::{Algorithm, HashAlg, PrivateKey};
    check(async {
        let key =
            Arc::new(PrivateKey::random(&mut rand::rng(), Algorithm::Rsa { hash: None }).unwrap());
        for (hash, expected) in [(HashAlg::Sha256, 2), (HashAlg::Sha512, 4)] {
            let agent = Agent::new(vec![Identity::plain(key.clone())]);
            let stats = agent.stats.clone();
            let fixture = Fixture::new(ServerOptions {
                key: Some(key.public_key().clone()),
                rsa_hash: Some(hash),
                ..Default::default()
            })
            .await;
            with_factory(move |_, _| Ok(agent.connect()), fixture.test(None)).await.unwrap();
            assert_eq!(*stats.flags.lock().unwrap(), vec![expected]);
        }
    });
}

#[test]
fn unavailable_empty_and_rejected_agents_fall_back_to_password() {
    check(async {
        for identities in [None, Some(Vec::new()), Some(vec![Identity::plain(key())])] {
            let agent = identities.map(Agent::new);
            let fixture = Fixture::new(ServerOptions {
                password: Some("draft".into()),
                ..Default::default()
            })
            .await;
            with_factory(
                move |_, _| {
                    agent.as_ref().map(Agent::connect).ok_or_else(|| "fixture unavailable".into())
                },
                fixture.test(Some("draft")),
            )
            .await
            .unwrap();
            assert_eq!(fixture.stats.passwords.load(Ordering::SeqCst), 1);
        }
    });
}

#[test]
fn formal_connection_falls_back_to_a_resolved_disk_key_after_agent_rejection() {
    check(async {
        let key = key();
        let mut fixture = Fixture::new(ServerOptions {
            key: Some(key.public_key().clone()),
            ..Default::default()
        })
        .await;
        let path = fixture.directory.path().join("disk-key");
        std::fs::write(
            &path,
            key.to_openssh(russh::keys::ssh_key::LineEnding::LF).unwrap().as_bytes(),
        )
        .unwrap();
        fixture.route.destination.identity_files.push(path);
        let agent = Agent::new(vec![Identity::plain(self::key())]);
        with_factory(move |_, _| Ok(agent.connect()), fixture.formal()).await.unwrap();
        assert_eq!(fixture.stats.accepted.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.stats.offered.lock().unwrap().len(), 2);
    });
}

#[test]
fn explicit_disk_key_precedes_agent_discovery() {
    check(async {
        let key = key();
        let mut fixture = Fixture::new(ServerOptions {
            key: Some(key.public_key().clone()),
            ..Default::default()
        })
        .await;
        let path = fixture.directory.path().join("explicit-key");
        std::fs::write(
            &path,
            key.to_openssh(russh::keys::ssh_key::LineEnding::LF).unwrap().as_bytes(),
        )
        .unwrap();
        fixture.route.profile.private_keys.push(path);
        with_factory(|_, _| panic!("explicit key should have completed authentication"), async {
            fixture.formal().await.unwrap();
            fixture.test(None).await.unwrap();
        })
        .await;
    });
}

#[test]
fn strict_modes_never_query_agents_or_send_an_unrelated_draft_password() {
    check(async {
        for mode in
            [SshAuthMode::Password, SshAuthMode::PublicKey, SshAuthMode::KeyboardInteractive]
        {
            let mut fixture = Fixture::new(ServerOptions {
                password: Some("draft".into()),
                ..Default::default()
            })
            .await;
            fixture.route.profile.auth = mode;
            let result = with_factory(
                |_, _| panic!("strict mode must not query an agent"),
                fixture.test(Some("draft")),
            )
            .await;
            assert_eq!(result.is_ok(), mode == SshAuthMode::Password);
            assert_eq!(
                fixture.stats.passwords.load(Ordering::SeqCst),
                usize::from(mode == SshAuthMode::Password)
            );
        }
    });
}

#[test]
fn hanging_identity_enumeration_has_a_short_deadline_and_can_fall_back() {
    check(async {
        let agent = Agent { behavior: AgentBehavior::HangIdentities, ..Default::default() };
        let fixture =
            Fixture::new(ServerOptions { password: Some("draft".into()), ..Default::default() })
                .await;
        let started = tokio::time::Instant::now();
        with_factory(move |_, _| Ok(agent.connect()), fixture.test(Some("draft"))).await.unwrap();
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(fixture.stats.passwords.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn signature_refusal_or_disconnect_is_fatal_to_both_connection_paths() {
    check(async {
        for behavior in [AgentBehavior::RefuseSignature, AgentBehavior::DisconnectSignature] {
            let key = key();
            let agent = Agent { behavior, ..Agent::new(vec![Identity::plain(key.clone())]) };
            let stats = agent.stats.clone();
            let fixture = Fixture::new(ServerOptions {
                key: Some(key.public_key().clone()),
                password: Some("draft".into()),
                ..Default::default()
            })
            .await;
            with_factory(move |_, _| Ok(agent.connect()), async {
                let error = fixture.formal().await.unwrap_err();
                assert!(error.to_string().contains("agent") || error.to_string().contains("Agent"));
                fixture.closed(1).await;
                assert!(fixture.test(Some("draft")).await.is_err());
                fixture.closed(2).await;
            })
            .await;
            assert_eq!(stats.signs.load(Ordering::SeqCst), 2);
            assert_eq!(fixture.stats.passwords.load(Ordering::SeqCst), 0);
        }
    });
}

#[test]
fn signature_timeout_discards_test_connection_without_password_fallback() {
    check(async {
        let key = key();
        let agent = Agent {
            behavior: AgentBehavior::HangSignature,
            ..Agent::new(vec![Identity::plain(key.clone())])
        };
        let fixture = Fixture::new(ServerOptions {
            key: Some(key.public_key().clone()),
            password: Some("draft".into()),
            ..Default::default()
        })
        .await;
        let started = tokio::time::Instant::now();
        let error = with_factory(move |_, _| Ok(agent.connect()), fixture.test(Some("draft")))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("SSH agent authentication/signing timed out"));
        assert!(started.elapsed() < Duration::from_secs(15));
        fixture.closed(1).await;
        assert_eq!(fixture.stats.passwords.load(Ordering::SeqCst), 0);
    });
}

#[test]
fn public_selectors_prioritize_agent_identity_without_loading_a_private_key() {
    check(async {
        let key = key();
        let mut fixture = Fixture::new(ServerOptions {
            key: Some(key.public_key().clone()),
            ..Default::default()
        })
        .await;
        let path = fixture.directory.path().join("only-public-selector");
        std::fs::write(public_selector(&path), key.public_key().to_openssh().unwrap()).unwrap();
        fixture.route.profile.private_keys.push(path.clone());
        assert!(!path.exists());
        let mut keys: Vec<_> = (0..7).map(|_| Identity::plain(self::key())).collect();
        keys.push(Identity::plain(key.clone()));
        let agent = Agent::new(keys);
        with_factory(move |_, _| Ok(agent.connect()), fixture.test(None)).await.unwrap();
        let offered = fixture.stats.offered.lock().unwrap();
        assert_eq!(offered.len(), 1, "unrelated keys must not exhaust server auth attempts first");
        assert_eq!(offered[0].key_data(), key.public_key().key_data());
    });
}

#[test]
fn identity_enumeration_does_not_silently_stop_after_five_keys() {
    check(async {
        let key = key();
        let fixture = Fixture::new(ServerOptions {
            key: Some(key.public_key().clone()),
            ..Default::default()
        })
        .await;
        let mut identities: Vec<_> = (0..5).map(|_| Identity::plain(self::key())).collect();
        identities.push(Identity::plain(key));
        let agent = Agent::new(identities);
        with_factory(move |_, _| Ok(agent.connect()), fixture.test(None)).await.unwrap();
        assert_eq!(fixture.stats.offered.lock().unwrap().len(), 6);
    });
}

#[test]
fn partial_agent_authentication_continues_to_the_second_factor() {
    check(async {
        let key = key();
        let mut fixture = Fixture::new(ServerOptions {
            key: Some(key.public_key().clone()),
            password: Some("factor-two".into()),
            second_factor: true,
            ..Default::default()
        })
        .await;
        fixture
            .route
            .destination
            .identity_files
            .push(fixture.directory.path().join("must-not-load-after-agent-factor"));
        let agent = Agent::new(vec![Identity::plain(key)]);
        let stats = agent.stats.clone();
        with_factory(move |_, _| Ok(agent.connect()), fixture.test(Some("factor-two")))
            .await
            .unwrap();
        assert_eq!(stats.signs.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.stats.passwords.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn jump_and_target_rank_their_own_agent_identities() {
    check(async {
        let jump_key = key();
        let target_key = key();
        let mut target = Fixture::new(ServerOptions {
            key: Some(target_key.public_key().clone()),
            ..Default::default()
        })
        .await;
        let mut jump = Fixture::new(ServerOptions {
            key: Some(jump_key.public_key().clone()),
            forward: Some((target.route.destination.host.clone(), target.route.destination.port)),
            ..Default::default()
        })
        .await;
        for (fixture, key) in [(&mut jump, &jump_key), (&mut target, &target_key)] {
            let path = fixture.directory.path().join("selector");
            std::fs::write(public_selector(&path), key.public_key().to_openssh().unwrap()).unwrap();
            fixture.route.destination.identity_files.push(path);
        }
        target.route.transport = RouteTransport::Jump(Box::new(ResolvedRoute {
            destination: jump.route.destination.clone(),
            profile: jump.route.profile.clone(),
            transport: RouteTransport::Direct,
            known_hosts_path: jump.route.known_hosts_path.clone(),
        }));
        let agent = Agent::new(vec![
            Identity::plain(target_key.clone()),
            Identity::plain(jump_key.clone()),
        ]);
        let stats = agent.stats.clone();
        with_factory(move |_, _| Ok(agent.connect()), async {
            target.formal().await.unwrap();
            target.test(None).await.unwrap();
        })
        .await;
        assert_eq!(
            stats.queries.load(Ordering::SeqCst),
            4,
            "each host discovers identities afresh"
        );
        assert_eq!(
            jump.stats.offered.lock().unwrap()[0].key_data(),
            jump_key.public_key().key_data()
        );
        assert_eq!(
            target.stats.offered.lock().unwrap()[0].key_data(),
            target_key.public_key().key_data()
        );
        crate::ssh_session::connection_pool().lock().await.remove(&jump.route.pool_key());
    });
}

#[test]
fn discovery_results_distinguish_unavailable_empty_and_rejected() {
    check(async {
        for (identities, expected) in [
            (None, Attempt::Unavailable),
            (Some(Vec::new()), Attempt::Empty),
            (
                Some(vec![Identity::plain(key()), Identity::plain(key())]),
                Attempt::Rejected { available: 2, attempted: 2 },
            ),
        ] {
            let agent = identities.map(Agent::new);
            let fixture = Fixture::new(ServerOptions::default()).await;
            let result = with_factory(
                move |endpoint, _| {
                    if endpoint != ENDPOINTS[0] {
                        return Err("no second endpoint".into());
                    }
                    agent.as_ref().map(Agent::connect).ok_or_else(|| "unavailable".into())
                },
                fixture.attempt(ENDPOINTS),
            )
            .await
            .unwrap();
            assert_eq!(result, expected);
            assert_eq!(fixture.stats.passwords.load(Ordering::SeqCst), 0);
        }
    });
}

#[test]
fn cancelling_formal_authentication_while_signing_closes_the_transport() {
    check(async {
        let identity = key();
        let agent = Agent {
            behavior: AgentBehavior::HangSignature,
            ..Agent::new(vec![Identity::plain(identity.clone())])
        };
        let stats = agent.stats.clone();
        let fixture = Fixture::new(ServerOptions {
            key: Some(identity.public_key().clone()),
            ..Default::default()
        })
        .await;
        let mut connection =
            Box::pin(with_factory(move |_, _| Ok(agent.connect()), fixture.formal()));
        tokio::select! {
            _ = &mut connection => panic!("fixture must wait for the agent to sign"),
            _ = async {
                while stats.signs.load(Ordering::SeqCst) == 0 { tokio::time::sleep(Duration::from_millis(5)).await; }
            } => {},
        }
        drop(connection);
        fixture.closed(1).await;
        assert_eq!(fixture.stats.passwords.load(Ordering::SeqCst), 0);
    });
}

#[test]
fn selector_reading_is_bounded_and_accepts_public_keys_or_certificates() {
    check(async {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("selector.pub");
        let identity = key();
        assert!(read_public_selector(&path).await.is_none());
        assert!(read_public_selector(directory.path()).await.is_none());
        for invalid in
            [b"not a public key".to_vec(), vec![b' '; PUBLIC_KEY_BYTES as usize + 1], vec![255]]
        {
            std::fs::write(&path, invalid).unwrap();
            assert!(read_public_selector(&path).await.is_none());
        }
        let mut public = identity.public_key().clone();
        public.set_comment("a comment does not change identity matching");
        std::fs::write(&path, public.to_openssh().unwrap()).unwrap();
        assert_eq!(
            read_public_selector(&path).await.unwrap().key_data(),
            identity.public_key().key_data()
        );
        let AgentIdentity::Certificate { certificate, .. } =
            Identity::certificate(identity.clone()).advertised
        else {
            unreachable!()
        };
        std::fs::write(&path, certificate.to_openssh().unwrap()).unwrap();
        assert_eq!(
            read_public_selector(&path).await.unwrap().key_data(),
            identity.public_key().key_data()
        );
        assert_eq!(public_selector(&path), path);
        assert_eq!(public_selector(Path::new("key.name")), PathBuf::from("key.name.pub"));
    });
}

#[test]
fn selector_order_is_explicit_then_resolved_and_other_identities_stay_stable() {
    check(async {
        let directory = tempfile::tempdir().unwrap();
        let explicit = key();
        let resolved = key();
        let other = key();
        let last = key();
        let paths = [directory.path().join("explicit"), directory.path().join("resolved")];
        for (path, identity) in paths.iter().zip([&explicit, &resolved]) {
            std::fs::write(public_selector(path), identity.public_key().to_openssh().unwrap())
                .unwrap();
        }
        let preferred = preferred_keys(paths.iter()).await;
        let mut identities = [
            Identity::plain(other.clone()),
            Identity::plain(resolved.clone()),
            Identity::plain(last.clone()),
            Identity::certificate(explicit.clone()),
        ]
        .map(|identity| identity.advertised);
        rank_identities(&mut identities, &preferred);
        for (identity, expected) in identities.iter().zip([explicit, resolved, other, last]) {
            assert_eq!(identity.public_key().key_data(), expected.public_key().key_data());
        }
    });
}

#[test]
fn endpoint_fallback_order_and_duplicate_identity_suppression() {
    check(async {
        for first in [None, Some(Agent::default()), Some(Agent::new(vec![Identity::plain(key())]))]
        {
            let accepted = key();
            let second = Agent::new(vec![Identity::plain(accepted.clone())]);
            let fixture = Fixture::new(ServerOptions {
                key: Some(accepted.public_key().clone()),
                ..Default::default()
            })
            .await;
            let calls = Arc::new(Mutex::new(Vec::new()));
            let captured = calls.clone();
            let result = with_factory(
                move |endpoint, _| {
                    captured.lock().unwrap().push(endpoint);
                    match endpoint {
                        Endpoint::OpenSsh => {
                            first.as_ref().map(Agent::connect).ok_or_else(|| "unavailable".into())
                        },
                        Endpoint::Pageant => Ok(second.connect()),
                        _ => unreachable!("fixture has only two candidates"),
                    }
                },
                fixture.attempt(&[Endpoint::OpenSsh, Endpoint::Pageant]),
            )
            .await
            .unwrap();
            assert_eq!(result, Attempt::Authenticated);
            assert_eq!(*calls.lock().unwrap(), vec![Endpoint::OpenSsh, Endpoint::Pageant]);
        }
        let rejected = Identity::plain(key());
        let first = Agent::new(vec![rejected.clone()]);
        let second = Agent::new(vec![rejected]);
        let fixture =
            Fixture::new(ServerOptions { password: Some("draft".into()), ..Default::default() })
                .await;
        let result = with_factory(
            move |endpoint, _| {
                Ok(match endpoint {
                    Endpoint::OpenSsh => first.connect(),
                    Endpoint::Pageant => second.connect(),
                    _ => unreachable!("fixture has only two candidates"),
                })
            },
            fixture.attempt(&[Endpoint::OpenSsh, Endpoint::Pageant]),
        )
        .await
        .unwrap();
        assert_eq!(result, Attempt::Rejected { available: 2, attempted: 1 });
        assert_eq!(fixture.stats.offered.lock().unwrap().len(), 1);
    });
}
