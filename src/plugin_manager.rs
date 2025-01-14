#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use chm_core_define::plugin_define::Event;
use chm_core_define::PluginError;
use chm_core_define::{plugin_define::Plugin, Result};
use libloading::Library;
use std::collections::{HashMap, HashSet};

use std::path::{Path, PathBuf};
/// 插件狀態
#[derive(Debug, Clone, PartialEq)]
#[allow(unused)]
/// 插件狀態枚舉，用來表示插件的不同狀態
pub enum PluginState {
    /// 插件尚未加載
    Unloaded,
    /// 插件已加載，但未啟用
    Loaded,
    /// 插件已啟用
    Enabled,
    /// 插件已禁用
    Disabled,
    /// 插件處於錯誤狀態，附帶錯誤訊息
    Error(String),
}
/// 插件條目，表示單個插件的詳細資訊
#[derive(Debug)]
struct PluginEntry {
    /// 插件的具體實例
    plugin: Box<dyn Plugin>,
    /// 動態庫的句柄，用於管理插件的生命周期
    library: Library,
    /// 插件當前的狀態      
    state: PluginState,
}

/// 事件系統，用於管理事件的訂閱和通知
#[derive(Debug)]
struct EventBus {
    /// 每個事件對應的訂閱插件集合
    subscribers: HashMap<String, HashSet<String>>, // event_name -> plugin_names
}
#[allow(unused)]
impl EventBus {
    /// 創建新的事件總線
    fn new() -> Self {
        Self {
            subscribers: HashMap::new(),
        }
    }
    #[allow(clippy::unwrap_or_default)]
    /// 訂閱事件
    /// - `event`: 要訂閱的事件名稱
    /// - `plugin`: 訂閱此事件的插件名稱
    fn subscribe(&mut self, event: &str, plugin: &str) {
        self.subscribers
            .entry(event.to_string())
            .or_insert_with(HashSet::new)
            .insert(plugin.to_string());
    }
    /// 取消訂閱事件
    /// - `event`: 要取消的事件名稱
    /// - `plugin`: 要取消訂閱的插件名稱
    fn unsubscribe(&mut self, event: &str, plugin: &str) {
        if let Some(subscribers) = self.subscribers.get_mut(event) {
            subscribers.remove(plugin);
        }
    }
    /// 獲取某事件的所有訂閱者
    /// - `event`: 事件名稱
    /// - 返回值: 訂閱此事件的插件名稱列表
    fn get_subscribers(&self, event: &str) -> Vec<String> {
        self.subscribers
            .get(event)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }
}

/// 插件管理器，用於管理插件的加載、啟用、禁用和事件通知
#[derive(Debug)]
pub struct PluginManager {
    /// 插件的集合，鍵為插件名稱
    plugins: HashMap<String, PluginEntry>,
    /// 插件目錄的路徑
    plugin_dir: PathBuf,
    /// 事件總線
    event_bus: EventBus,
}
#[allow(unused)]
impl PluginManager {
    /// 創建新的插件管理器
    /// - `plugin_dir`: 插件目錄路徑
    pub fn new<P: AsRef<Path>>(plugin_dir: P) -> Self {
        Self {
            plugins: HashMap::new(),
            plugin_dir: plugin_dir.as_ref().to_path_buf(),
            event_bus: EventBus::new(),
        }
    }
    /// 加載單個插件
    /// - `path`: 插件檔案的路徑
    /// - 返回值: 成功或失敗的結果
    pub fn load_plugin(&mut self, path: &Path) -> Result<()> {
        unsafe {
            let lib = Library::new(path)
                .map_err(|e| PluginError::LoadError(format!("Failed to load library: {}", e)))?;

            // 獲取創建插件函數
            let create_plugin: libloading::Symbol<fn() -> Box<dyn Plugin>> =
                lib.get(b"create_plugin").map_err(|e| {
                    PluginError::LoadError(format!("Failed to get create_plugin symbol: {}", e))
                })?;

            // 創建插件實例
            let plugin = create_plugin();
            let name = plugin.name().to_string();
            // 調用加載鉤子

            plugin.on_load()?;
            // 註冊事件訂閱
            for event in plugin.subscribed_events() {
                self.event_bus.subscribe(&event, &name);
            }
            println!("Loaded plugin: {} v{}", name, plugin.version());
            self.plugins.insert(
                name.clone(),
                PluginEntry {
                    plugin,
                    library: lib,
                    state: PluginState::Loaded,
                },
            );
            self.enable_plugin(name.as_str())?;
            Ok(())
        }
    }
    /// 啟用插件
    /// - `name`: 插件名稱
    /// - 返回值: 成功或失敗的結果
    pub fn enable_plugin(&mut self, name: &str) -> Result<()> {
        let ret = self.plugins.get_mut(name);
        if let Some(entry) = ret {
            if entry.state == PluginState::Enabled {
                return Ok(());
            }
            if entry.state == PluginState::Loaded {
                entry.plugin.on_enable()?;
                entry.state = PluginState::Enabled;
                println!("Enabled plugin: {}", name);
                return Ok(());
            }
        }
        Err(PluginError::EnableError("Can't enable plugin".into()))
    }
    /// 禁用插件
    /// - `name`: 插件名稱
    /// - 返回值: 成功或失敗的結果
    pub fn disable_plugin(&mut self, name: &str) -> Result<()> {
        let ret = self.plugins.get_mut(name);
        if let Some(entry) = ret {
            if entry.state == PluginState::Disabled {
                return Ok(());
            }
            if entry.state == PluginState::Enabled {
                entry.plugin.on_disable()?;
                entry.state = PluginState::Disabled;
                println!("Disabled plugin: {}", name);
                return Ok(());
            }
        }
        Err(PluginError::DisableError("Can't disable plugin".into()))
    }
    /// 卸載插件
    /// - `name`: 插件名稱
    /// - 返回值: 成功或失敗的結果
    pub fn unload_plugin(&mut self, name: &str) -> Result<()> {
        // 先檢查插件是否存在
        if let Some(entry) = self.plugins.get(name) {
            // 1. 創建一個事件訂閱的副本
            let events = entry.plugin.subscribed_events();

            // 2. 執行禁用邏輯
            self.disable_plugin(name)?;

            // 3. 取消訂閱所有事件
            for event in events {
                self.event_bus.unsubscribe(&event, name);
            }

            // 4. 獲取插件實例並執行卸載操作
            if let Some(mut entry) = self.plugins.remove(name) {
                // 調用卸載鉤子
                entry.plugin.on_unload()?;

                // 執行標準卸載程序
                unsafe {
                    if let Ok(unload_plugin) = entry.library.get::<fn()>(b"unload_plugin") {
                        unload_plugin();
                    }
                }
                println!("Unloaded plugin: {}", name);
            }
        }
        Ok(())
    }

    /// 發送事件
    /// - `event`: 要發送的事件
    /// - 返回值: 成功或失敗的結果
    // pub fn broadcast_event(&self, event: Event) -> Result<()> {
    //     let subscribers = self.event_bus.get_subscribers(&event.name);
    //     // 根據優先級排序
    //     let mut subscribers: Vec<_> = subscribers
    //         .iter()
    //         .filter_map(|name| {
    //             self.plugins.get(name).and_then(|entry| {
    //                 if entry.state == PluginState::Enabled {
    //                     Some((name, entry))
    //                 } else {
    //                     None
    //                 }
    //             })
    //         })
    //         .collect();
    //     subscribers.sort_by_key(|(_, entry)| {
    //         entry.plugin.subscribed_events().len() // 簡單用訂閱數量作為優先級
    //     });
    //     // 依序發送事件
    //     for (name, entry) in subscribers {
    //         if let Err(e) = entry.plugin.handle_event(&event) {
    //             println!("Error handling event in plugin {}: {}", name, e);
    //         }
    //     }
    //     Ok(())
    // }
    // pub fn broadcast_event(&self, event: Event) -> Result<()> {
    //     let subscribers = self.event_bus.get_subscribers(&event.name);

    //     for name in subscribers {
    //         if let Some(entry) = self.plugins.get(&name) {
    //             if entry.state == PluginState::Enabled {
    //                 // 處理事件並檢查是否有回應事件
    //                 if let Some(response_event) = entry.plugin.handle_event(&event)? {
    //                     // 遞歸發送回應事件
    //                     self.broadcast_event(response_event)?;
    //                 }
    //             }
    //         }
    //     }

    //     Ok(())
    // }

    /// 載入所有插件
    /// - 返回值: 成功或失敗的結果
    pub fn load_all_plugins(&mut self) -> Result<()> {
        let mut errors = Vec::new();

        // 驗證插件目錄存在且可讀取
        if !self.plugin_dir.exists() {
            return Err(PluginError::LoadError(
                "Plugin directory does not exist".into(),
            ));
        }

        // 讀取目錄項目
        let dir_entries = match std::fs::read_dir(&self.plugin_dir) {
            Ok(entries) => entries,
            Err(e) => {
                return Err(PluginError::LoadError(format!(
                    "Failed to read plugin directory: {}",
                    e
                )))
            }
        };

        // 處理每個插件檔案
        for entry in dir_entries {
            match entry {
                Ok(entry) => {
                    let path = entry.path();

                    // 驗證是否為有效的插件檔案
                    if !self.is_valid_plugin_file(&path) {
                        continue;
                    }

                    // 嘗試載入插件
                    if let Err(e) = self.load_plugin(&path) {
                        let error_msg = format!("Failed to load plugin from {:?}: {}", path, e);
                        errors.push(error_msg.clone());
                        eprintln!("{}", error_msg);
                    }
                }
                Err(e) => {
                    let error_msg = format!("Failed to read directory entry: {}", e);
                    errors.push(error_msg.clone());
                    eprintln!("{}", error_msg);
                }
            }
        }

        // 如果有任何錯誤,收集並回傳
        if !errors.is_empty() {
            return Err(PluginError::LoadError(format!(
                "Failed to load some plugins:\n{}",
                errors.join("\n")
            )));
        }

        Ok(())
    }

    fn is_valid_plugin_file(&self, path: &Path) -> bool {
        // 基本副檔名檢查
        let is_valid_extension = path.extension().map_or(false, |ext| match ext.to_str() {
            #[cfg(target_os = "windows")]
            Some("dll") => true,
            #[cfg(target_os = "linux")]
            Some("so") => true,
            #[cfg(target_os = "macos")]
            Some("dylib") => true,
            _ => false,
        });

        if !is_valid_extension {
            return false;
        }

        // 確保檔案存在且可讀取
        if !path.exists() || !path.is_file() {
            return false;
        }

        // 檢查檔案權限
        if let Ok(metadata) = path.metadata() {
            #[cfg(unix)]
            return metadata.permissions().mode() & 0o111 != 0;
            #[cfg(not(unix))]
            return metadata.permissions().readonly() == false;
        }

        false
    }
    /// 獲取插件
    /// - `name`: 插件名稱
    /// - 返回值: 插件實例
    pub fn get_plugin(&self, name: &str) -> Option<&dyn Plugin> {
        self.plugins.get(name).map(|entry| entry.plugin.as_ref())
    }
    /// 獲取所有插件
    /// - 返回值: 插件列表
    pub fn get_all_plugins(&self) -> Vec<(&str, &str, &str)> {
        self.plugins
            .values()
            .map(|entry| {
                (
                    entry.plugin.name(),
                    entry.plugin.version(),
                    entry.plugin.description(),
                )
            })
            .collect()
    }
    /// 卸載所有插件
    /// - 返回值: 成功或失敗的結果
    pub fn unload_all_plugins(&mut self) -> Result<()> {
        let names: Vec<_> = self.plugins.keys().cloned().collect();
        for name in names {
            if let Err(e) = self.unload_plugin(&name) {
                eprintln!("Error unloading plugin {}: {}", name, e);
            }
        }
        Ok(())
    }
}
/// 插件管理器的析構函數，用於在管理器被刪除時卸載所有插件
impl Drop for PluginManager {
    fn drop(&mut self) {
        if let Err(e) = self.unload_all_plugins() {
            eprintln!("Error unloading plugins during drop: {}", e);
        }
    }
}
