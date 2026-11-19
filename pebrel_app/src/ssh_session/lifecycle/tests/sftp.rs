use super::*;

pub(super) struct Peer;
impl russh_sftp::server::Handler for Peer {
    type Error = russh_sftp::protocol::StatusCode;
    fn unimplemented(&self) -> Self::Error {
        russh_sftp::protocol::StatusCode::OpUnsupported
    }
}

#[test]
fn rejected_sftp_reports_the_subsystem_and_releases_the_channel() {
    check(async {
        let mut fixture = Fixture::new(Mode::RejectSftp).await;
        let acquired = fixture.connect().await;
        let mut shell = own_channel(acquired.session.channel_open_session().await.unwrap());
        for _ in 0..3 {
            let result = tokio::time::timeout(
                Duration::from_secs(1),
                super::super::super::sftp_on_session(&acquired.session),
            )
            .await;
            let error = result.expect("a rejected subsystem must fail immediately").err().unwrap();
            assert!(error.to_string().contains("rejected the SFTP subsystem"), "{error}");
            assert_eq!(fixture.data.recv().await.unwrap(), b"sftp-request");
            assert_eq!(fixture.data.recv().await.unwrap(), b"closed");
        }
        assert!(!acquired.session.is_closed());
        shell.finish().await.unwrap();
        fixture.forget(&acquired.session).await;
    });
}

#[test]
fn cancelling_sftp_negotiation_closes_only_its_channel() {
    check(async {
        let mut fixture = Fixture::new(Mode::HangSftp).await;
        let acquired = fixture.connect().await;
        let mut shell = own_channel(acquired.session.channel_open_session().await.unwrap());
        let mut work = Box::pin(super::super::super::sftp_on_session(&acquired.session));
        tokio::select! {
            _ = &mut work => panic!("fixture withholds the subsystem reply"),
            request = fixture.data.recv() => assert_eq!(request.unwrap(), b"sftp-request"),
        }
        drop(work);
        assert_eq!(fixture.data.recv().await.unwrap(), b"closed");
        assert!(!acquired.session.is_closed());
        shell.finish().await.unwrap();
        fixture.forget(&acquired.session).await;
    });
}

#[test]
fn accepted_sftp_can_reopen_on_the_original_pane_transport() {
    check(async {
        let fixture = Fixture::new(Mode::AcceptSftp).await;
        let acquired = fixture.connect().await;
        let (scope, pane) =
            super::super::super::pane_connection::PaneConnectionScope::new(&acquired.session);
        for _ in 0..3 {
            let sftp = pane.open_sftp().await.unwrap();
            sftp.close().await.unwrap();
        }
        drop(scope);
        assert!(pane.open_sftp().await.is_err());
        assert!(!acquired.session.is_closed());
        fixture.forget(&acquired.session).await;
    });
}
