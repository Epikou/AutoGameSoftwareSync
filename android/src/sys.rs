//! Appels au systeme Android par JNI (permission « Acces a tous les fichiers », stockages, modele de l'appareil, langue du
//! systeme, ouverture d'une page web). Android 11 (API 30) minimum : seule l'autorisation `MANAGE_EXTERNAL_STORAGE` compte.
//! Hors Android (tests sur PC), chaque fonction renvoie une valeur neutre. Appele depuis le fil d'interface uniquement.
use std::path::PathBuf;

pub const PACKAGE: &str = "io.github.epikou.agss";
/// Racine du stockage partage de l'utilisateur (identique sur tous les Android recents ; `/sdcard` y renvoie).
pub const PRIMARY_STORAGE: &str = "/storage/emulated/0";

#[cfg(target_os = "android")]
mod imp {
    use super::*;
    use jni::objects::{JObject, JString, JValue};
    use jni::{JNIEnv, JavaVM};

    /// Execute `f` avec l'environnement JNI du fil courant et l'activite (contexte) de l'application.
    fn with_env<R>(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<R>) -> Option<R> {
        let actx = ndk_context::android_context();
        let vm = unsafe { JavaVM::from_raw(actx.vm() as *mut _) }.ok()?;
        let mut env = vm.attach_current_thread().ok()?;
        let activity = unsafe { JObject::from_raw(actx.context() as jni::sys::jobject) };
        let r = f(&mut env, &activity);
        // Une exception Java non effacee ferait planter le prochain appel : on la vide et on journalise.
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
            log::warn!("exception Java effacee");
            return None;
        }
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                log::warn!("JNI : {}", e);
                None
            }
        }
    }

    fn jstring_to_string(env: &mut JNIEnv, o: JObject) -> Option<String> {
        if o.is_null() {
            return None;
        }
        let js = JString::from(o);
        env.get_string(&js).ok().map(|s| s.into())
    }

    pub fn sdk_int() -> i32 {
        with_env(|env, _| env.get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?.i()).unwrap_or(0)
    }

    pub fn device_model() -> String {
        with_env(|env, _| {
            let o = env.get_static_field("android/os/Build", "MODEL", "Ljava/lang/String;")?.l()?;
            Ok(jstring_to_string(env, o).unwrap_or_default())
        })
        .unwrap_or_default()
    }

    pub fn system_language() -> String {
        with_env(|env, _| {
            let loc = env.call_static_method("java/util/Locale", "getDefault", "()Ljava/util/Locale;", &[])?.l()?;
            let o = env.call_method(&loc, "getLanguage", "()Ljava/lang/String;", &[])?.l()?;
            Ok(jstring_to_string(env, o).unwrap_or_default())
        })
        .unwrap_or_default()
    }

    /// « Acces a tous les fichiers » (Android 11+). En doute (exception JNI) : refuse.
    pub fn has_all_files_access() -> bool {
        with_env(|env, _| env.call_static_method("android/os/Environment", "isExternalStorageManager", "()Z", &[])?.z()).unwrap_or(false)
    }

    fn start_activity(env: &mut JNIEnv, act: &JObject, action: &str, data: Option<&str>) -> jni::errors::Result<()> {
        let a = env.new_string(action)?;
        let intent = match data {
            Some(d) => {
                let ds = env.new_string(d)?;
                let uri = env.call_static_method("android/net/Uri", "parse", "(Ljava/lang/String;)Landroid/net/Uri;", &[JValue::Object(&ds)])?.l()?;
                env.new_object("android/content/Intent", "(Ljava/lang/String;Landroid/net/Uri;)V", &[JValue::Object(&a), JValue::Object(&uri)])?
            }
            None => env.new_object("android/content/Intent", "(Ljava/lang/String;)V", &[JValue::Object(&a)])?,
        };
        env.call_method(act, "startActivity", "(Landroid/content/Intent;)V", &[JValue::Object(&intent)])?;
        Ok(())
    }

    /// Ouvre l'ecran systeme de l'autorisation pour cette appli (repli : l'ecran general si la surcouche n'a pas le premier).
    pub fn request_all_files_access() {
        let ok = with_env(|env, act| start_activity(env, act, "android.settings.MANAGE_APP_ALL_FILES_ACCESS_PERMISSION", Some(&format!("package:{}", PACKAGE)))).is_some();
        if !ok {
            with_env(|env, act| start_activity(env, act, "android.settings.MANAGE_ALL_FILES_ACCESS_PERMISSION", None));
        }
    }

    pub fn open_url(url: &str) -> bool {
        with_env(|env, act| start_activity(env, act, "android.intent.action.VIEW", Some(url))).is_some()
    }

    pub fn move_task_to_back() {
        with_env(|env, act| env.call_method(act, "moveTaskToBack", "(Z)Z", &[JValue::Bool(1)]).map(|_| ()));
    }

    pub fn external_storage_dir() -> Option<PathBuf> {
        with_env(|env, _| {
            let f = env.call_static_method("android/os/Environment", "getExternalStorageDirectory", "()Ljava/io/File;", &[])?.l()?;
            let o = env.call_method(&f, "getAbsolutePath", "()Ljava/lang/String;", &[])?.l()?;
            Ok(jstring_to_string(env, o).map(PathBuf::from))
        })
        .flatten()
    }
}

#[cfg(not(target_os = "android"))]
mod imp {
    use super::*;
    pub fn sdk_int() -> i32 {
        0
    }
    pub fn device_model() -> String {
        String::new()
    }
    pub fn system_language() -> String {
        String::new()
    }
    pub fn has_all_files_access() -> bool {
        true
    }
    pub fn request_all_files_access() {}
    pub fn open_url(_url: &str) -> bool {
        false
    }
    pub fn move_task_to_back() {}
    pub fn external_storage_dir() -> Option<PathBuf> {
        None
    }
}

pub use imp::*;

/// Racines de stockage visibles : stockage interne puis cartes SD (`/storage/XXXX-XXXX`), lisibles seulement.
pub fn storage_roots() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let primary = external_storage_dir().filter(|p| p.is_dir()).unwrap_or_else(|| PathBuf::from(PRIMARY_STORAGE));
    out.push(primary);
    out.extend(sd_cards(std::path::Path::new("/storage")));
    out
}

/// Cartes SD (et cles USB) montees sous `/storage` : tout dossier lisible sauf `emulated` et `self`.
pub fn sd_cards(storage: &std::path::Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(storage) {
        for e in rd.filter_map(|e| e.ok()) {
            let n = e.file_name().to_string_lossy().to_string();
            if n == "emulated" || n == "self" || n.starts_with('.') {
                continue;
            }
            let p = e.path();
            if p.is_dir() && std::fs::read_dir(&p).is_ok() {
                v.push(p);
            }
        }
    }
    v.sort();
    v
}

/// Nom court d'une racine de stockage pour l'ecran (interne / carte SD XXXX-XXXX).
pub fn root_label(p: &std::path::Path) -> Option<String> {
    let s = p.to_string_lossy();
    if s.starts_with(PRIMARY_STORAGE) || s == "/sdcard" {
        None
    } else {
        p.file_name().map(|n| n.to_string_lossy().to_string())
    }
}

/// La permission est-elle reellement utilisable ? (le systeme dit oui ET la racine se lit). Hors Android : toujours oui.
pub fn storage_readable() -> bool {
    if cfg!(not(target_os = "android")) {
        return true;
    }
    if !has_all_files_access() {
        log::warn!("autorisation « Acces a tous les fichiers » refusee par le systeme");
        return false;
    }
    match std::fs::read_dir(PRIMARY_STORAGE) {
        Ok(_) => true,
        Err(e) => {
            log::warn!("{} illisible malgre l'autorisation : {}", PRIMARY_STORAGE, e);
            false
        }
    }
}
