//! Copy-and-paste registration text for MCP clients.
//!
//! Koushi never edits another tool's configuration; the settings page shows
//! these snippets with the bundled launcher's path filled in. The text is not
//! product prose: it is a command or a config fragment, identical in every
//! locale.

use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationClient {
    ClaudeCode,
    Codex,
    Pi,
    DeepseekHarness,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SnippetKind {
    /// Run in a terminal.
    Command,
    /// Add to a YAML configuration file.
    Yaml,
    /// Add to a JSON configuration file.
    Json,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Registration {
    pub client: RegistrationClient,
    /// The name the assistant presents, and the name the user approves.
    pub client_name: String,
    pub kind: SnippetKind,
    pub text: String,
    /// Where a configuration fragment goes, when it is a file.
    pub config_hint: Option<String>,
}

/// The snippets for one launcher path. `windows` selects the quoting rules of
/// the platform the path belongs to.
pub fn registrations(launcher: &str, windows: bool) -> Vec<Registration> {
    let shell_path = if windows {
        format!("\"{launcher}\"")
    } else {
        posix_quote(launcher)
    };
    let command = |client, client_name: &str, prefix: &str| Registration {
        client,
        client_name: client_name.to_owned(),
        kind: SnippetKind::Command,
        text: format!("{prefix} -- {shell_path} --client-name {client_name}"),
        config_hint: None,
    };
    vec![
        command(
            RegistrationClient::ClaudeCode,
            "claude-code",
            "claude mcp add --scope user koushi",
        ),
        command(RegistrationClient::Codex, "codex", "codex mcp add koushi"),
        command(RegistrationClient::Pi, "pi", "pi mcp add koushi"),
        Registration {
            client: RegistrationClient::DeepseekHarness,
            client_name: "deepseek-harness".to_owned(),
            kind: SnippetKind::Yaml,
            text: format!(
                "- id: mcp-koushi\n  name: '@deepseek-ai/dsh-mcp-client'\n  config:\n    serverName: koushi\n    transport: stdio\n    command: {}\n    args: ['--client-name', 'deepseek-harness']\n",
                yaml_quote(launcher)
            ),
            config_hint: Some("~/.dsh/profiles/<profile>/cordis.patch.yml".to_owned()),
        },
        Registration {
            client: RegistrationClient::Other,
            client_name: "my-client".to_owned(),
            kind: SnippetKind::Json,
            text: other_json(launcher),
            config_hint: None,
        },
    ]
}

/// POSIX single quotes; an embedded `'` closes, escapes, and reopens.
fn posix_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// A YAML single-quoted scalar; `'` is doubled.
fn yaml_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn other_json(launcher: &str) -> String {
    let value = serde_json::json!({
        "mcpServers": {
            "koushi": {
                "command": launcher,
                "args": ["--client-name", "my-client"]
            }
        }
    });
    serde_json::to_string_pretty(&value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(registrations: &[Registration], client: RegistrationClient) -> &str {
        &registrations
            .iter()
            .find(|registration| registration.client == client)
            .expect("client")
            .text
    }

    #[test]
    fn commands_quote_a_posix_path_with_spaces_and_quotes() {
        let all = registrations("/Apps/Kou shi's/koushi-mcp", false);
        assert_eq!(
            text(&all, RegistrationClient::ClaudeCode),
            "claude mcp add --scope user koushi -- '/Apps/Kou shi'\\''s/koushi-mcp' --client-name claude-code"
        );
        assert_eq!(
            text(&all, RegistrationClient::Pi),
            "pi mcp add koushi -- '/Apps/Kou shi'\\''s/koushi-mcp' --client-name pi"
        );
    }

    #[test]
    fn windows_paths_use_double_quotes() {
        let all = registrations(
            r"C:\Users\Member 1\AppData\Local\Koushi\koushi-mcp.exe",
            true,
        );
        assert_eq!(
            text(&all, RegistrationClient::Codex),
            r#"codex mcp add koushi -- "C:\Users\Member 1\AppData\Local\Koushi\koushi-mcp.exe" --client-name codex"#
        );
    }

    #[test]
    fn json_and_yaml_escape_the_path() {
        let all = registrations(r"C:\Kou'shi\koushi-mcp.exe", true);
        let json: serde_json::Value =
            serde_json::from_str(text(&all, RegistrationClient::Other)).expect("valid json");
        assert_eq!(
            json["mcpServers"]["koushi"]["command"],
            r"C:\Kou'shi\koushi-mcp.exe"
        );
        assert!(
            text(&all, RegistrationClient::DeepseekHarness)
                .contains(r"command: 'C:\Kou''shi\koushi-mcp.exe'")
        );
    }

    #[test]
    fn every_client_has_a_distinct_name() {
        let all = registrations("/usr/bin/koushi-mcp", false);
        let mut names: Vec<&str> = all.iter().map(|r| r.client_name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 5);
    }
}
