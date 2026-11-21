"""Native profile setup and independent transcript oracles for offline CLIs."""
import json


PROFILES = {"claude": ".claude", "pi": ".pi/agent", "omp": ".omp/agent",
            "grok": ".grok", "codebuddy": ".codebuddy"}


def write_json(path, data):
    path.write_text(json.dumps(data), encoding="utf-8")


def configure(agent, home, work_dir, url):
    profile = home / PROFILES[agent]
    profile.mkdir(parents=True)
    if agent in ("pi", "omp"):
        model = {"baseUrl": url + "/v1", "api": "openai-completions", "apiKey": "fixture",
                 "models": [{"id": "fixture", "name": "Fixture"}]}
        models = "models.json" if agent == "pi" else "models.yml"
        write_json(profile / models, {"providers": {"pebrelfixture": model}})
        write_json(profile / "settings.json", {"defaultProvider": "pebrelfixture", "defaultModel": "fixture",
                                               "defaultProjectTrust": "always"})
        return {"PI_CODING_AGENT_DIR": str(profile), "PI_OFFLINE": "1",
                "OMP_SKIP_SETUP": "1", "OMP_SKIP_VERSION_CHECK": "1"}
    if agent == "claude":
        onboarding = {"hasCompletedOnboarding": True, "theme": "dark",
                      "projects": {str(work_dir): {"hasTrustDialogAccepted": True,
                                                    "projectOnboardingSeenCount": 5}},
                      "customApiKeyResponses": {"approved": ["fixture-key"], "rejected": []}}
        write_json(home / ".claude.json", onboarding)
        write_json(profile / ".claude.json", onboarding)
        return {"ANTHROPIC_BASE_URL": url, "ANTHROPIC_API_KEY": "fixture-key",
                "CLAUDE_CONFIG_DIR": str(profile), "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
                "DISABLE_AUTOUPDATER": "1"}
    if agent == "grok":
        (profile / "config.toml").write_text(
            '[models]\ndefault="fixture"\n[model.fixture]\nmodel="fixture"\n'
            f'base_url="{url}/v1"\napi_backend="chat_completions"\nenv_key="XAI_API_KEY"\n',
            encoding="utf-8")
        return {"GROK_HOME": str(profile), "XAI_API_KEY": "fixture-key",
                "GROK_MODELS_BASE_URL": url + "/v1", "GROK_XAI_API_BASE_URL": url + "/v1"}
    return {"CODEBUDDY_BASE_URL": url + "/v1", "CODEBUDDY_CONFIG_DIR": str(profile),
            "CODEBUDDY_API_KEY": "fixture-key"}


def command_for(agent, pane_index):
    if agent in ("pi", "omp"):
        return f"{agent} --model pebrelfixture/fixture"
    return "cbc" if agent == "codebuddy" and pane_index == 1 else agent


def has_text(content, marker):
    if isinstance(content, str):
        return content == marker
    return isinstance(content, list) and any(
        isinstance(part, dict) and part.get("text") == marker for part in content
    )


def native_match(agent, file, rows, session_id, marker):
    if agent in ("pi", "omp"):
        header = next((row for row in rows if row.get("type") == "session"), {})
        if header.get("id") != session_id:
            return False
    elif agent == "grok":
        if session_id not in file.parts and session_id != file.stem:
            return False
        marker = f"<user_query>\n{marker}\n</user_query>"
    seen = False
    for row in rows:
        if agent in ("claude", "codebuddy") and row.get("sessionId") != session_id:
            continue
        # CodeBuddy stores the assistant's role and content at the top level;
        # its nested message contains token usage.
        message = row if agent == "codebuddy" else (row.get("message") or row)
        role = message.get("role") or row.get("type")
        content = message.get("content")
        if role == "user":
            seen = has_text(content, marker)
        elif seen and role == "assistant" and has_text(content, "Native recovery fixture reply."):
            return True
    return False


def provider_confirmed(home, agent, session_id, marker):
    if not session_id:
        return False
    profile = home / PROFILES[agent]
    directory = profile / ("projects" if agent in ("claude", "codebuddy") else "sessions")
    for file in directory.rglob("*.jsonl"):
        if file.is_symlink():
            continue
        try:
            rows = [json.loads(line) for line in file.read_text(encoding="utf-8").splitlines()
                    if line.strip()]
        except (OSError, ValueError):
            continue
        if native_match(agent, file, rows, session_id, marker):
            return True
    return False
