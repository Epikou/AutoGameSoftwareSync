//! LISTE NOIRE DURE (non contournable) : jamais de cles, identifiants, jetons, cookies, mots de passe ni profils de
//! navigateurs dans une sauvegarde. Appliquee (1) a TOUS les ensembles de fichiers au scan (`store::Target::accepts`) et
//! (2) a la validation des profils communautaires (rejet) : aucun profil ne peut contourner ces regles.

/// Dossiers qui ne sont jamais sauvegardes, a n'importe quel niveau.
const DIRS: &[&str] = &[".ssh", ".gnupg", ".aws", ".azure", ".kube", ".docker", ".password-store", ".gcloud", ".config/gcloud", "keychains"];

/// Fichiers (motifs `*` et `?`, minuscules, sur le nom seul) qui ne sont jamais sauvegardes.
const FILES: &[&str] = &[
    "id_rsa*", "id_dsa*", "id_ecdsa*", "id_ed25519*", "*.pem", "*.pfx", "*.p12", "*.ppk", "*.kdbx", "*.keystore", "*.jks", ".git-credentials", ".netrc", ".npmrc", ".pypirc",
    "credentials", "credentials.*", "login data*", "cookies", "cookies.*", "web data*", "logins.json", "key3.db", "key4.db", "cert8.db", "cert9.db", "*.gpg", "*.asc", "known_hosts",
    "authorized_keys", "secrets.json", "secrets.yml", "secrets.yaml", "token", "tokens.json", "*.token", "service-token*",
];

fn glob(p: &str, t: &str) -> bool {
    crate::store::glob_match(p, t)
}

/// Un chemin relatif (separateur `/`, minuscules) est-il interdit ? Conservateur : seulement des fichiers / dossiers d'identifiants.
pub fn rel_is_sensitive(rel_lower: &str) -> bool {
    let comps: Vec<&str> = rel_lower.split('/').filter(|c| !c.is_empty()).collect();
    for (i, c) in comps.iter().enumerate() {
        let is_last = i + 1 == comps.len();
        if DIRS.iter().any(|d| !d.contains('/') && d == c) {
            return true;
        }
        if is_last && FILES.iter().any(|p| glob(p, c)) {
            return true;
        }
        if !is_last && (c.eq_ignore_ascii_case("credentials") || c.eq_ignore_ascii_case("cookies")) {
            return true;
        }
    }
    false
}

/// Mots et dossiers qui rendent un TEXTE de profil (chemin, motif, etiquette) inacceptable : validation des profils (large).
const WORDS: &[&str] = &[
    "password", "passwd", "motdepasse", "credential", "token", "secret", "cookie", "login data", "logins", "apikey", "api_key", "api-key", "private key", "privatekey", "id_rsa", "id_ed25519",
    ".ssh", ".gnupg", ".aws", ".azure", ".kube", "keychain", "wallet", "keyring", "authorized_keys", "known_hosts", ".git-credentials", ".netrc", ".npmrc", "oauth", "session store", "sessionstore",
];

/// Profils de navigateurs et gestionnaires de mots de passe (sous-chaines de chemins en minuscules, separateur `/`).
const BROWSERS: &[&str] = &[
    "google/chrome", "chromium", "microsoft/edge", "mozilla/firefox", "mozilla/profiles", "bravesoftware", "opera software", "vivaldi", "thunderbird", "tor browser", "keepass", "bitwarden",
    "1password", "lastpass", "dashlane", "microsoft/credentials", "microsoft/protect", "microsoft/vault",
];

/// Un texte de profil est-il interdit ? Retourne la raison. `text` : chemin-gabarit, motif ou etiquette.
pub fn text_is_sensitive(text: &str) -> Option<String> {
    let t = text.to_lowercase().replace('\\', "/");
    for w in WORDS {
        if t.contains(w) {
            return Some(format!("« {} » est interdit (identifiants / cles / jetons)", w));
        }
    }
    for b in BROWSERS {
        if t.contains(b) {
            return Some(format!("profil de navigateur ou de gestionnaire de mots de passe interdit (« {} »)", b));
        }
    }
    if rel_is_sensitive(&t) {
        return Some("fichier ou dossier d'identifiants interdit".to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fichiers_et_dossiers_interdits() {
        for r in [".ssh/id_rsa", "a/b/.ssh/config", "x/id_ed25519.pub", "cert.pem", "app/login data", "Profile/Cookies", "a/.aws/credentials", "k.kdbx", "x/key4.db"] {
            assert!(rel_is_sensitive(&r.to_lowercase()), "{}", r);
        }
        for r in ["settings.json", "saves/slot1.sav", "keybindings.json", "config/config.ini", "tokenizer_levels.sav"] {
            assert!(!rel_is_sensitive(r), "{}", r);
        }
    }

    #[test]
    fn textes_de_profil() {
        assert!(text_is_sensitive("%APPDATA%\\Code\\User\\secrets.json").is_some());
        assert!(text_is_sensitive("%LOCALAPPDATA%\\Google\\Chrome\\User Data").is_some());
        assert!(text_is_sensitive("%USERPROFILE%\\.ssh").is_some());
        assert!(text_is_sensitive("*password*").is_some());
        assert!(text_is_sensitive("%APPDATA%\\Code\\User\\settings.json").is_none());
        assert!(text_is_sensitive("%APPDATA%\\EldenRing").is_none());
    }
}
