mod config;

use config::Config;
use jni::JNIEnv;
use jni::objects::{JClass, JString, JValue};
use jni::sys::JNINativeMethod;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use zygisk_api::ZygiskModule;
use zygisk_api::api::v4::ZygiskOption;
use zygisk_api::api::{V4, ZygiskApi};

// 全局状态:存储当前应用的伪装属性
static FAKE_PROPS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

// 全局状态:标记是否使用了 full 模式（需要保持模块加载）
static IS_FULL_MODE: Mutex<bool> = Mutex::new(false);

// Resetprop 模式：记录当前处于伪装状态的应用
static ACTIVE_RESET_SESSION: Mutex<Option<ActiveResetSession>> = Mutex::new(None);

// 原始 native_get 函数指针类型
type OriginalNativeGet = unsafe extern "C" fn(
    env: *mut jni::sys::JNIEnv,
    class: jni::sys::jclass,
    key: jni::sys::jstring,
    def: jni::sys::jstring,
) -> jni::sys::jstring;

// 存储原始 native_get 函数指针
static ORIGINAL_NATIVE_GET: Mutex<Option<OriginalNativeGet>> = Mutex::new(None);

#[derive(Default)]
struct MyModule;

impl ZygiskModule for MyModule {
    type Api = V4;

    fn on_load(&self, _api: ZygiskApi<V4>, _env: JNIEnv) {
        // 默认只记录错误，减少日志输出以提高隐蔽性
        // 可以通过配置文件的 debug = true 启用详细日志
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Error) // 默认只记录错误
                .with_tag("DeviceFaker"),
        );
    }

    fn pre_app_specialize(
        &self,
        mut api: ZygiskApi<V4>,
        mut env: JNIEnv,
        args: &mut <V4 as zygisk_api::raw::ZygiskRaw>::AppSpecializeArgs,
    ) {
        let mut inner = || -> anyhow::Result<()> {
            // 获取包名
            let package_name: String = env
                .get_string(args.nice_name)
                .map_err(|e| anyhow::anyhow!("Failed to get package name: {}", e))?
                .into();

            // 如果有其他应用遗留的 resetprop 伪装，先恢复
            Self::restore_previous_resetprop_if_needed(&mut api, &package_name)?;

            // 读取配置文件
            let config_path = "/data/adb/device_faker/config/config.toml";

            // 检查文件是否存在
            if !std::path::Path::new(config_path).exists() {
                api.set_option(ZygiskOption::DlCloseModuleLibrary);
                return Ok(());
            }

            let config_content = match std::fs::read_to_string(config_path) {
                Ok(content) => content,
                Err(e) => {
                    error!("Failed to read config: {}", e);
                    api.set_option(ZygiskOption::DlCloseModuleLibrary);
                    return Ok(());
                }
            };

            let config = Config::from_toml(&config_content)?;

            // 根据配置启用或禁用详细日志
            if config.debug {
                log::set_max_level(log::LevelFilter::Info);
                info!("Debug mode enabled");
                info!(
                    "Config loaded with {} apps and {} templates",
                    config.apps.len(),
                    config.templates.len()
                );
            }

            // 获取合并后的配置（会同时查找直接配置和模板配置）
            let merged_config = match config.get_merged_config(&package_name) {
                Some(cfg) => cfg,
                None => {
                    // 应用不在配置中
                    if config.debug {
                        info!("App {} not in config, unloading module", package_name);
                    }
                    api.set_option(ZygiskOption::DlCloseModuleLibrary);
                    return Ok(());
                }
            };

            // 获取工作模式
            let mode = &merged_config.mode;
            if config.debug {
                info!("Using mode: {} for app: {}", mode, package_name);
            }

            // Hook Build 类字段（两种模式都需要）
            Self::hook_build_fields(&mut env, &merged_config)?;
            if config.debug {
                info!("Build fields hooked successfully");
            }

            // 根据模式决定是否 Hook SystemProperties
            match mode.as_str() {
                "lite" => {
                    // 轻量模式：只修改 Build 类，完成后卸载模块
                    if config.debug {
                        info!("Lite mode: only Build fields hooked, unloading module");
                    }
                    *IS_FULL_MODE.lock().unwrap() = false;
                    api.set_option(ZygiskOption::DlCloseModuleLibrary);
                }
                "full" => {
                    // 完整模式：Hook Build + SystemProperties
                    if config.debug {
                        info!("Full mode: hooking SystemProperties");
                    }

                    // 标记为 full 模式，防止模块被卸载
                    *IS_FULL_MODE.lock().unwrap() = true;

                    // 构建属性映射表并存储
                    let prop_map = Config::build_merged_property_map(&merged_config);
                    if config.debug {
                        info!("Property map created with {} entries", prop_map.len());
                    }
                    *FAKE_PROPS.lock().unwrap() = Some(prop_map);

                    // Hook SystemProperties
                    Self::hook_system_properties(&mut api, &env)?;
                    if config.debug {
                        info!("SystemProperties hooked successfully, module will stay loaded");
                    }
                }
                "resetprop" => {
                    // Resetprop 模式：使用 companion 进程执行 resetprop
                    if config.debug {
                        info!("Resetprop mode: using companion process");
                    }

                    // 构建属性映射表
                    let prop_map = Config::build_merged_property_map(&merged_config);

                    // 通过 companion 执行 resetprop
                    Self::spoof_system_props_via_companion(&mut api, &prop_map, &package_name)?;

                    if config.debug {
                        info!("Resetprop spoofing completed");
                    }

                    // 完成后卸载模块（不需要保持 Hook）
                    *IS_FULL_MODE.lock().unwrap() = false;
                    api.set_option(ZygiskOption::DlCloseModuleLibrary);
                }
                _ => {
                    // 未知模式,回退到默认的 lite 模式
                    error!(
                        "Mode '{}' not fully supported, falling back to 'lite' mode",
                        mode
                    );
                    if config.debug {
                        info!("Lite mode (fallback): only Build fields hooked, unloading module");
                    }
                    *IS_FULL_MODE.lock().unwrap() = false;
                    api.set_option(ZygiskOption::DlCloseModuleLibrary);
                }
            }

            Ok(())
        };

        if let Err(e) = inner() {
            error!("Error: {:?}", e);
        }
    }

    fn post_app_specialize(
        &self,
        mut api: ZygiskApi<V4>,
        _env: JNIEnv,
        _args: &<V4 as zygisk_api::raw::ZygiskRaw>::AppSpecializeArgs,
    ) {
        // 只在非 full 模式下卸载模块
        // full 模式下，Hook 函数需要保持在内存中，否则会导致崩溃
        let is_full_mode = *IS_FULL_MODE.lock().unwrap();
        if !is_full_mode {
            // lite 模式：可以安全卸载
            api.set_option(ZygiskOption::DlCloseModuleLibrary);
        }
        // full 模式：不卸载，保持 Hook 函数可用
    }

    fn pre_server_specialize(
        &self,
        mut api: ZygiskApi<V4>,
        _env: JNIEnv,
        _args: &mut <V4 as zygisk_api::raw::ZygiskRaw>::ServerSpecializeArgs,
    ) {
        api.set_option(ZygiskOption::DlCloseModuleLibrary);
    }
}

impl MyModule {
    /// Hook Build 类的静态字段
    fn hook_build_fields(
        env: &mut JNIEnv,
        merged_config: &config::MergedAppConfig,
    ) -> anyhow::Result<()> {
        // 查找 android.os.Build 类
        let build_class = env
            .find_class("android/os/Build")
            .map_err(|e| anyhow::anyhow!("Failed to find Build class: {}", e))?;

        // 只修改配置中存在且非空的字段
        if let Some(manufacturer) = &merged_config.manufacturer
            && !manufacturer.is_empty()
        {
            Self::set_build_field(env, &build_class, "MANUFACTURER", manufacturer)?;
        }

        if let Some(brand) = &merged_config.brand
            && !brand.is_empty()
        {
            Self::set_build_field(env, &build_class, "BRAND", brand)?;
        }

        if let Some(model) = &merged_config.model
            && !model.is_empty()
        {
            Self::set_build_field(env, &build_class, "MODEL", model)?;
        }

        if let Some(device) = &merged_config.device
            && !device.is_empty()
        {
            Self::set_build_field(env, &build_class, "DEVICE", device)?;
        }

        if let Some(product) = &merged_config.product
            && !product.is_empty()
        {
            Self::set_build_field(env, &build_class, "PRODUCT", product)?;
        }

        if let Some(fingerprint) = &merged_config.fingerprint
            && !fingerprint.is_empty()
        {
            Self::set_build_field(env, &build_class, "FINGERPRINT", fingerprint)?;
        }

        Ok(())
    }

    /// 设置 Build 类的字段值
    fn set_build_field(
        env: &mut JNIEnv,
        build_class: &JClass,
        field_name: &str,
        value: &str,
    ) -> anyhow::Result<()> {
        // 获取字段 ID
        let field_id = env
            .get_static_field_id(build_class, field_name, "Ljava/lang/String;")
            .map_err(|e| anyhow::anyhow!("Failed to get field ID for {}: {}", field_name, e))?;

        // 创建新的字符串值
        let new_value = env
            .new_string(value)
            .map_err(|e| anyhow::anyhow!("Failed to create string for {}: {}", value, e))?;

        // 设置静态字段
        env.set_static_field(build_class, field_id, JValue::Object(&new_value))
            .map_err(|e| anyhow::anyhow!("Failed to set field {}: {}", field_name, e))?;

        Ok(())
    }

    /// Hook SystemProperties.native_get 方法
    fn hook_system_properties(api: &mut ZygiskApi<V4>, env: &JNIEnv) -> anyhow::Result<()> {
        // 定义要 Hook 的 JNI 方法
        let mut methods = [JNINativeMethod {
            name: c"native_get".as_ptr() as *mut u8,
            signature: c"(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;".as_ptr()
                as *mut u8,
            fnPtr: native_get_hook as *mut std::ffi::c_void,
        }];

        // Hook SystemProperties 类的 native_get 方法
        use jni::strings::JNIStr;
        let class_name = unsafe { JNIStr::from_ptr(c"android/os/SystemProperties".as_ptr()) };

        unsafe {
            // 使用 unsafe_clone 来复制 JNIEnv
            api.hook_jni_native_methods(env.unsafe_clone(), class_name, &mut methods);
        }

        // 保存原始函数指针
        let original_fn_ptr = unsafe {
            std::mem::transmute::<*mut std::ffi::c_void, OriginalNativeGet>(methods[0].fnPtr)
        };
        *ORIGINAL_NATIVE_GET.lock().unwrap() = Some(original_fn_ptr);

        Ok(())
    }

    fn spoof_system_props_via_companion(
        api: &mut ZygiskApi<V4>,
        prop_map: &HashMap<String, String>,
        package_name: &str,
    ) -> anyhow::Result<()> {
        if prop_map.is_empty() {
            return Ok(());
        }

        let request = CompanionRequest::Apply(ResetpropSessionRequest {
            pid: std::process::id(),
            props: prop_map.clone(),
        });

        let response = Self::send_companion_command(api, &request)?;
        if response.status != 0 {
            anyhow::bail!(
                response
                    .message
                    .unwrap_or_else(|| "companion resetprop failed".to_string())
            );
        }

        if let Some(backups) = response.backups {
            *ACTIVE_RESET_SESSION.lock().unwrap() = Some(ActiveResetSession {
                package: package_name.to_string(),
                backups,
            });
        } else {
            warn!("Companion did not return property backups; automatic restore may be skipped");
        }

        Ok(())
    }

    fn send_companion_command(
        api: &mut ZygiskApi<V4>,
        request: &CompanionRequest,
    ) -> anyhow::Result<CompanionResponse> {
        let payload = serde_json::to_vec(request)?;
        let response = api
            .with_companion(|stream| -> anyhow::Result<CompanionResponse> {
                stream.write_all(&(payload.len() as u32).to_le_bytes())?;
                stream.write_all(&payload)?;
                stream.flush()?;

                let mut len_buf = [0u8; 4];
                stream.read_exact(&mut len_buf)?;
                let resp_len = u32::from_le_bytes(len_buf) as usize;
                let mut resp_buf = vec![0u8; resp_len];
                stream.read_exact(&mut resp_buf)?;

                let resp = serde_json::from_slice::<CompanionResponse>(&resp_buf)?;
                Ok(resp)
            })
            .map_err(|e| anyhow::anyhow!("Failed to talk to companion: {e}"))??;

        Ok(response)
    }

    fn restore_previous_resetprop_if_needed(
        api: &mut ZygiskApi<V4>,
        current_package: &str,
    ) -> anyhow::Result<()> {
        let mut guard = ACTIVE_RESET_SESSION.lock().unwrap();
        let pending = guard.take();

        match pending {
            Some(session) if session.package != current_package => {
                if let Err(e) = Self::restore_props_via_companion(api, &session.backups) {
                    error!("Failed to restore previous resetprop session: {e}");
                }
            }
            other => {
                *guard = other;
            }
        }

        Ok(())
    }

    fn restore_props_via_companion(
        api: &mut ZygiskApi<V4>,
        backups: &HashMap<String, String>,
    ) -> anyhow::Result<()> {
        if backups.is_empty() {
            return Ok(());
        }

        let request = CompanionRequest::Restore(RestoreRequest {
            props: backups.clone(),
        });

        let response = Self::send_companion_command(api, &request)?;
        if response.status != 0 {
            anyhow::bail!(
                response
                    .message
                    .unwrap_or_else(|| "companion restore failed".to_string())
            );
        }

        Ok(())
    }
}

zygisk_api::register_module!(MyModule);

// SystemProperties.native_get Hook 函数
unsafe extern "C" fn native_get_hook(
    env: *mut jni::sys::JNIEnv,
    class: jni::sys::jclass,
    key: jni::sys::jstring,
    def: jni::sys::jstring,
) -> jni::sys::jstring {
    // 创建 JNIEnv 包装器
    let mut env_wrapper = match unsafe { JNIEnv::from_raw(env) } {
        Ok(e) => e,
        Err(_) => return def,
    };

    // 获取属性名
    let key_jstring = unsafe { JString::from_raw(key) };
    let key_str = match env_wrapper.get_string(&key_jstring) {
        Ok(s) => s,
        Err(_) => return def,
    };
    let key_string: String = key_str.into();

    // 检查是否需要伪装此属性
    let result = {
        let fake_props = FAKE_PROPS.lock().unwrap();
        if let Some(props) = fake_props.as_ref() {
            props.get(&key_string).and_then(|fake_value| {
                env_wrapper
                    .new_string(fake_value)
                    .ok()
                    .map(|s| s.into_raw())
            })
        } else {
            None
        }
    };

    // 如果有伪装值，返回；否则调用原始函数
    if let Some(fake_result) = result {
        return fake_result;
    }

    // 未匹配的属性：调用原始 native_get 函数获取真实值
    let original_native_get = ORIGINAL_NATIVE_GET.lock().unwrap();
    if let Some(orig_fn) = *original_native_get {
        return unsafe { orig_fn(env, class, key, def) };
    }

    // 如果原始函数不可用（不应该发生），返回默认值
    def
}

// Companion entry point
fn handle_companion_request(stream: &mut std::os::unix::net::UnixStream) {
    let response = match read_companion_request(stream) {
        Ok(CompanionRequest::Apply(request)) => match apply_resetprop_session(request) {
            Ok(backups) => CompanionResponse::ok_with_backups(backups),
            Err(err) => {
                error!("Companion failed to apply resetprop session: {err}");
                CompanionResponse::err(err.to_string())
            }
        },
        Ok(CompanionRequest::Restore(request)) => match restore_properties(request) {
            Ok(_) => CompanionResponse::ok(),
            Err(err) => {
                error!("Companion failed to restore properties: {err}");
                CompanionResponse::err(err.to_string())
            }
        },
        Err(err) => {
            error!("Companion failed to parse request: {err}");
            CompanionResponse::err("invalid request")
        }
    };

    if let Err(e) = write_companion_response(stream, &response) {
        warn!("Failed to write companion response: {}", e);
    }
}

fn read_companion_request(
    stream: &mut std::os::unix::net::UnixStream,
) -> anyhow::Result<CompanionRequest> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let payload_len = u32::from_le_bytes(len_buf) as usize;
    if payload_len == 0 {
        anyhow::bail!("empty request payload");
    }

    let mut payload = vec![0u8; payload_len];
    stream.read_exact(&mut payload)?;
    let request = serde_json::from_slice::<CompanionRequest>(&payload)?;
    Ok(request)
}

fn write_companion_response(
    stream: &mut std::os::unix::net::UnixStream,
    response: &CompanionResponse,
) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(response)?;
    stream.write_all(&(bytes.len() as u32).to_le_bytes())?;
    stream.write_all(&bytes)?;
    stream.flush()?;
    Ok(())
}

fn apply_resetprop_session(
    request: ResetpropSessionRequest,
) -> anyhow::Result<HashMap<String, String>> {
    if request.props.is_empty() {
        return Ok(HashMap::new());
    }

    let resetprop_path = find_resetprop_path()
        .ok_or_else(|| anyhow::anyhow!("resetprop binary not found in known locations"))?;

    let mut backups = Vec::with_capacity(request.props.len());
    for key in request.props.keys() {
        let original = backup_property(key)?;
        backups.push(PropBackup {
            key: key.clone(),
            original_value: original,
        });
    }

    let backups_for_response: HashMap<String, String> = backups
        .iter()
        .map(|entry| (entry.key.clone(), entry.original_value.clone()))
        .collect();

    for (key, value) in &request.props {
        apply_resetprop(&resetprop_path, key, value)?;
    }

    spawn_restore_watcher(request.pid, backups, resetprop_path)?;

    Ok(backups_for_response)
}

fn restore_properties(request: RestoreRequest) -> anyhow::Result<()> {
    if request.props.is_empty() {
        return Ok(());
    }

    let resetprop_path = find_resetprop_path()
        .ok_or_else(|| anyhow::anyhow!("resetprop binary not found in known locations"))?;

    for (key, value) in request.props {
        apply_resetprop(&resetprop_path, &key, &value)?;
    }

    Ok(())
}

fn backup_property(key: &str) -> anyhow::Result<String> {
    let output = std::process::Command::new("getprop").arg(key).output()?;
    if !output.status.success() {
        anyhow::bail!("getprop failed for {key}");
    }

    let value = String::from_utf8_lossy(&output.stdout)
        .trim_end_matches(['\n', '\r'])
        .to_string();
    Ok(value)
}

fn apply_resetprop(path: &str, key: &str, value: &str) -> anyhow::Result<()> {
    let status = std::process::Command::new(path)
        .arg(key)
        .arg(value)
        .status()?;
    if !status.success() {
        anyhow::bail!("resetprop failed for {key}");
    }
    Ok(())
}

fn spawn_restore_watcher(
    pid: u32,
    backups: Vec<PropBackup>,
    resetprop_path: String,
) -> anyhow::Result<()> {
    unsafe {
        match libc::fork() {
            -1 => anyhow::bail!("fork failed: {}", std::io::Error::last_os_error()),
            0 => {
                if libc::setsid() == -1 {
                    libc::_exit(1);
                }
                wait_for_process_inactive(pid);
                for entry in backups {
                    if let Err(e) =
                        apply_resetprop(&resetprop_path, &entry.key, &entry.original_value)
                    {
                        error!(
                            "Failed to restore property {} for pid {}: {}",
                            entry.key, pid, e
                        );
                    }
                }
                libc::_exit(0);
            }
            _ => Ok(()),
        }
    }
}

fn wait_for_process_inactive(pid: u32) {
    let proc_path = format!("/proc/{pid}");
    loop {
        if !std::path::Path::new(&proc_path).exists() {
            break;
        }

        if !is_process_in_top_app(pid) {
            break;
        }

        thread::sleep(Duration::from_millis(500));
    }
}

fn is_process_in_top_app(pid: u32) -> bool {
    let cgroup_path = format!("/proc/{pid}/cgroup");
    match fs::read_to_string(&cgroup_path) {
        Ok(content) => content.lines().any(|line| line.contains("top-app")),
        Err(_) => true,
    }
}

#[derive(Clone)]
struct PropBackup {
    key: String,
    original_value: String,
}

fn find_resetprop_path() -> Option<String> {
    let possible_paths = [
        "/data/adb/ksu/bin/resetprop",
        "/data/adb/magisk/resetprop",
        "/debug_ramdisk/resetprop",
        "/data/adb/ap/bin/resetprop",
        "/system/bin/resetprop",
        "/vendor/bin/resetprop",
    ];

    for path in possible_paths {
        if std::path::Path::new(path).exists() {
            return Some(path.to_string());
        }
    }

    // Try `which resetprop`
    std::process::Command::new("which")
        .arg("resetprop")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|path| !path.is_empty() && std::path::Path::new(path).exists())
}

zygisk_api::register_companion!(handle_companion_request);

#[derive(Serialize, Deserialize, Debug)]
struct ResetpropSessionRequest {
    pid: u32,
    props: HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct RestoreRequest {
    props: HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "cmd", content = "payload")]
enum CompanionRequest {
    Apply(ResetpropSessionRequest),
    Restore(RestoreRequest),
}

#[derive(Serialize, Deserialize, Debug)]
struct CompanionResponse {
    status: i32,
    message: Option<String>,
    backups: Option<HashMap<String, String>>,
}

impl CompanionResponse {
    fn ok() -> Self {
        Self {
            status: 0,
            message: None,
            backups: None,
        }
    }

    fn err(msg: impl Into<String>) -> Self {
        Self {
            status: -1,
            message: Some(msg.into()),
            backups: None,
        }
    }

    fn ok_with_backups(backups: HashMap<String, String>) -> Self {
        Self {
            status: 0,
            message: None,
            backups: Some(backups),
        }
    }
}

struct ActiveResetSession {
    package: String,
    backups: HashMap<String, String>,
}
