use crate::{auth, cloud::AccountConfig, store::Store};
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sessions {
    pub active_id: Option<String>,
    pub accounts: Vec<AccountConfig>,
}

impl Sessions {
    pub fn load(store: &Store) -> Result<Self, String> {
        let Some(value) = store.load_account::<serde_json::Value>()? else {
            return Ok(Self::default());
        };
        if value.get("accounts").is_some() {
            let mut vault: Self = serde_json::from_value(value).map_err(|_| "会话存储格式无效")?;
            for config in &mut vault.accounts {
                config.migrate_saved_space();
            }
            return Ok(vault);
        }
        // Upgrade the encrypted single-account vault in place, preserving credentials.
        let mut config: AccountConfig =
            serde_json::from_value(value).map_err(|_| "保存的凭据格式无效")?;
        if config.id.is_empty() {
            config.id = uuid::Uuid::new_v4().to_string();
        }
        config.migrate_saved_space();
        let vault = Self {
            active_id: Some(config.id.clone()),
            accounts: vec![config],
        };
        vault.save(store)?;
        Ok(vault)
    }

    pub fn save(&self, store: &Store) -> Result<(), String> {
        store.save_account(self)
    }

    pub fn upsert(&mut self, mut config: AccountConfig) -> Result<(), String> {
        config.password.clear();
        config.sms_code.clear();
        if let Some(existing) = self.accounts.iter_mut().find(|entry| entry.id == config.id) {
            *existing = config.clone();
        } else {
            if self.accounts.len() >= 64 {
                return Err("最多保存 64 个账号，请先移除不再使用的会话".into());
            }
            self.accounts.push(config.clone());
        }
        self.active_id = Some(config.id);
        Ok(())
    }

    pub fn matching_id(&self, config: &AccountConfig) -> Option<String> {
        let key = identity(config);
        if key == "share:" {
            return None;
        }
        self.accounts
            .iter()
            .find(|entry| identity(entry) == key)
            .map(|entry| entry.id.clone())
    }
}

fn identity(config: &AccountConfig) -> String {
    let account = auth::account_from_authorization(&config.authorization)
        .unwrap_or_else(|_| config.username.clone());
    if account.is_empty() {
        format!("share:{}", config.link_id)
    } else {
        account
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sessions_preserve_other_credentials_and_clear_secrets() {
        let mut vault = Sessions::default();
        for id in ["a", "b"] {
            vault
                .upsert(AccountConfig {
                    id: id.into(),
                    username: id.into(),
                    password: "secret".into(),
                    sms_code: "123456".into(),
                    ..Default::default()
                })
                .unwrap();
        }
        vault
            .upsert(AccountConfig {
                id: "a".into(),
                name: "updated".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(vault.accounts.len(), 2);
        assert_eq!(vault.active_id.as_deref(), Some("a"));
        assert_eq!(vault.accounts[1].username, "b");
        assert!(vault
            .accounts
            .iter()
            .all(|config| config.password.is_empty() && config.sms_code.is_empty()));
    }
}
