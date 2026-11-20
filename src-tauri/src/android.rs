//! Android-only helpers that need the activity: screenshot protection
//! (`FLAG_SECURE`) and reading the clipboard. Both run on the Android UI
//! thread through the webview's JNI handle.

use std::time::Duration;

use jni::objects::JObject;
use jni::objects::JString;
use jni::objects::JValue;
use jni::JNIEnv;

/// `android.view.WindowManager.LayoutParams.FLAG_SECURE`
const FLAG_SECURE: i32 = 0x0000_2000;

/// Block (or allow again) screenshots and the recent-apps preview.
pub(crate) fn set_secure(window: &tauri::WebviewWindow, enabled: bool) -> Result<(), String> {
    window
        .with_webview(move |webview| {
            webview.jni_handle().exec(move |env, activity, _webview| {
                if apply_secure_flag(env, activity, enabled).is_err() {
                    let _ = env.exception_clear();
                }
            });
        })
        .map_err(|e| e.to_string())
}

fn apply_secure_flag(
    env: &mut JNIEnv,
    activity: &JObject,
    enabled: bool,
) -> jni::errors::Result<()> {
    let window = env
        .call_method(activity, "getWindow", "()Landroid/view/Window;", &[])?
        .l()?;
    let method = if enabled { "addFlags" } else { "clearFlags" };
    env.call_method(&window, method, "(I)V", &[JValue::Int(FLAG_SECURE)])?;
    Ok(())
}

/// Text currently on the clipboard, if any.
pub(crate) async fn read_clipboard(
    window: &tauri::WebviewWindow,
) -> Result<Option<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    window
        .with_webview(move |webview| {
            webview.jni_handle().exec(move |env, activity, _webview| {
                let text = match clipboard_text(env, activity) {
                    Ok(text) => text,
                    Err(_) => {
                        let _ = env.exception_clear();
                        None
                    }
                };
                let _ = tx.send(text);
            });
        })
        .map_err(|e| e.to_string())?;
    tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .map_err(|_| "Clipboard did not respond".to_string())?
        .map_err(|_| "Clipboard not available".to_string())
}

fn clipboard_text(env: &mut JNIEnv, activity: &JObject) -> jni::errors::Result<Option<String>> {
    let service = env.new_string("clipboard")?;
    let manager = env
        .call_method(
            activity,
            "getSystemService",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            &[JValue::Object(&service)],
        )?
        .l()?;
    if manager.is_null() {
        return Ok(None);
    }
    let clip = env
        .call_method(
            &manager,
            "getPrimaryClip",
            "()Landroid/content/ClipData;",
            &[],
        )?
        .l()?;
    if clip.is_null() {
        return Ok(None);
    }
    let count = env.call_method(&clip, "getItemCount", "()I", &[])?.i()?;
    if count < 1 {
        return Ok(None);
    }
    let item = env
        .call_method(
            &clip,
            "getItemAt",
            "(I)Landroid/content/ClipData$Item;",
            &[JValue::Int(0)],
        )?
        .l()?;
    if item.is_null() {
        return Ok(None);
    }
    let chars = env
        .call_method(
            &item,
            "coerceToText",
            "(Landroid/content/Context;)Ljava/lang/CharSequence;",
            &[JValue::Object(activity)],
        )?
        .l()?;
    if chars.is_null() {
        return Ok(None);
    }
    let string = env
        .call_method(&chars, "toString", "()Ljava/lang/String;", &[])?
        .l()?;
    let string = JString::from(string);
    let text: String = env.get_string(&string)?.into();
    Ok(Some(text))
}
