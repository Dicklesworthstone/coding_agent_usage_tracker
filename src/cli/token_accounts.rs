//! `caut token-accounts` command: manage named per-provider credentials.

use std::io::{IsTerminal as _, Read as _};
use std::path::Path;

use crate::cli::args::TokenAccountsCommand;
use crate::core::provider::Provider;
use crate::error::{CautError, Result};
use crate::storage::paths::AppPaths;
use crate::storage::token_accounts::{TokenAccount, TokenAccountStore, convert};

/// Execute a `token-accounts` subcommand.
///
/// # Errors
/// Returns an error for an unknown provider, an unreadable or invalid
/// accounts file, or a failed add/remove/activate.
pub fn execute(cmd: TokenAccountsCommand) -> Result<()> {
    let path = AppPaths::new().token_accounts_file();
    match cmd {
        TokenAccountsCommand::List { provider } => {
            let store = TokenAccountStore::load(&path)?;
            let provider = provider
                .as_deref()
                .map(Provider::from_cli_name)
                .transpose()?;
            print!("{}", render_list(&store, provider, &path));
            Ok(())
        }
        TokenAccountsCommand::Add {
            provider,
            label,
            token,
            activate,
        } => {
            let provider = Provider::from_cli_name(&provider)?;
            let token = match token {
                Some(token) => token,
                None => read_token_from_stdin()?,
            };
            let mut store = TokenAccountStore::load(&path)?;
            store.add(provider, TokenAccount::new(label.trim(), token.trim()))?;
            if activate {
                store.set_active(provider, &label)?;
            }
            store.save()?;
            println!(
                "Added {} token account '{}'{}",
                provider.display_name(),
                label.trim(),
                if activate { " (active)" } else { "" }
            );
            Ok(())
        }
        TokenAccountsCommand::Remove { provider, label } => {
            let provider = Provider::from_cli_name(&provider)?;
            let mut store = TokenAccountStore::load(&path)?;
            let removed = store.remove(provider, &label)?;
            store.save()?;
            println!(
                "Removed {} token account '{}'",
                provider.display_name(),
                removed.label
            );
            Ok(())
        }
        TokenAccountsCommand::Use { provider, label } => {
            let provider = Provider::from_cli_name(&provider)?;
            let mut store = TokenAccountStore::load(&path)?;
            store.set_active(provider, &label)?;
            store.save()?;
            println!(
                "Active {} token account is now '{}'",
                provider.display_name(),
                label.trim()
            );
            Ok(())
        }
        TokenAccountsCommand::Convert { from, to } => convert_accounts(&from, &to, &path),
    }
}

/// Read a credential from stdin (piped, or typed and ended with Enter).
fn read_token_from_stdin() -> Result<String> {
    let stdin = std::io::stdin();
    let mut token = String::new();
    if stdin.is_terminal() {
        eprint!("Paste the token and press Enter: ");
        stdin.read_line(&mut token)?;
    } else {
        stdin.lock().read_to_string(&mut token)?;
    }
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(CautError::Config(
            "No token given: pass --token or pipe it on stdin".to_string(),
        ));
    }
    Ok(token)
}

/// Render the account listing for one provider or all of them.
fn render_list(store: &TokenAccountStore, provider: Option<Provider>, path: &Path) -> String {
    use std::fmt::Write as _;

    let providers: Vec<Provider> = provider.map_or_else(|| Provider::ALL.to_vec(), |p| vec![p]);
    let mut out = String::new();
    let mut found_any = false;
    for provider in providers {
        let Some(data) = store
            .get_provider(provider)
            .filter(|d| !d.accounts.is_empty())
        else {
            continue;
        };
        found_any = true;
        let active = data.clamped_active_index();
        let _ = writeln!(out, "{}:", provider.display_name());
        for (index, account) in data.accounts.iter().enumerate() {
            let marker = if index == active { '*' } else { ' ' };
            let _ = writeln!(
                out,
                "  {marker} {:>2}. {:<20} {:<16} added {}",
                index + 1,
                account.label,
                account.masked_token(),
                account.added_at.format("%Y-%m-%d")
            );
        }
    }
    if found_any {
        out.push_str("\n* = active account (used when no --account is given)\n");
    } else {
        match provider {
            Some(p) => {
                let _ = writeln!(
                    out,
                    "No token accounts configured for {}.",
                    p.display_name()
                );
            }
            None => out.push_str("No token accounts configured.\n"),
        }
        let _ = writeln!(
            out,
            "Add one with: caut token-accounts add --provider <name> --label <label>\nToken accounts file: {}",
            path.display()
        );
    }
    out
}

/// Convert between `CodexBar`'s and caut's token account files.
fn convert_accounts(from: &str, to: &str, caut_path: &Path) -> Result<()> {
    let from_lower = from.to_lowercase();
    let to_lower = to.to_lowercase();
    for (role, value) in [("source", from), ("target", to)] {
        if !["codexbar", "caut"].contains(&value.to_lowercase().as_str()) {
            return Err(CautError::Config(format!(
                "Unknown {role} format '{value}'. Valid formats: codexbar, caut"
            )));
        }
    }
    let codexbar_path = AppPaths::codexbar_token_accounts_file().ok_or_else(|| {
        CautError::Config("CodexBar token accounts path not available (macOS only)".to_string())
    })?;
    let (src_path, dst_path, to_codexbar) = match (from_lower.as_str(), to_lower.as_str()) {
        ("codexbar", "caut") => (codexbar_path, caut_path.to_path_buf(), false),
        ("caut", "codexbar") => (caut_path.to_path_buf(), codexbar_path, true),
        _ => {
            return Err(CautError::Config(format!(
                "Cannot convert from '{from}' to '{to}' (same format)"
            )));
        }
    };
    if !src_path.exists() {
        return Err(CautError::Config(format!(
            "Source file not found: {}",
            src_path.display()
        )));
    }
    let data = convert::from_codexbar(&std::fs::read_to_string(&src_path)?)?;
    let output = if to_codexbar {
        convert::to_codexbar(&data)?
    } else {
        convert::to_caut(&data)?
    };
    if let Some(parent) = dst_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dst_path, &output)?;
    println!("Converted {} -> {}", src_path.display(), dst_path.display());
    println!("Providers converted: {}", data.providers.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_marks_active_and_masks_tokens() {
        let mut store = TokenAccountStore::empty();
        store
            .add(
                Provider::Zai,
                TokenAccount::new("personal", "zai-secret-token-1111"),
            )
            .unwrap();
        store
            .add(
                Provider::Zai,
                TokenAccount::new("team", "zai-secret-token-2222"),
            )
            .unwrap();
        store.set_active(Provider::Zai, "team").unwrap();
        let out = render_list(&store, None, Path::new("/tmp/x.json"));
        assert!(out.contains("z.ai:"));
        assert!(out.contains("*  2. team"));
        assert!(out.contains("   1. personal"));
        assert!(
            !out.contains("zai-secret-token"),
            "tokens are masked: {out}"
        );
        assert!(out.contains("…2222"));
    }

    #[test]
    fn list_explains_how_to_add_when_empty() {
        let store = TokenAccountStore::empty();
        let out = render_list(&store, Some(Provider::Cursor), Path::new("/tmp/x.json"));
        assert!(out.contains("No token accounts configured for Cursor."));
        assert!(out.contains("caut token-accounts add"));
        assert!(out.contains("/tmp/x.json"));
    }

    #[test]
    fn convert_rejects_unknown_and_same_formats() {
        let path = Path::new("/nonexistent/token-accounts.json");
        assert!(convert_accounts("bogus", "caut", path).is_err());
        assert!(convert_accounts("caut", "bogus", path).is_err());
    }
}
