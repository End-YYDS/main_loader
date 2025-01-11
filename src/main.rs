/// 插件管理器
mod plugin_manager;
use chm_core_define::{Event, PluginError, Result};
use plugin_manager::PluginManager;
use std::{collections::HashMap, path::Path};

fn main() -> Result<()> {
    // 創建插件目錄
    let plugin_dir = Path::new("./plugins");
    if !plugin_dir.exists() {
        std::fs::create_dir_all(plugin_dir).map_err(|e| {
            PluginError::LoadError(format!("Failed to create plugin directory: {}", e))
        })?;
    }

    // 創建插件管理器
    let mut manager = PluginManager::new(plugin_dir);

    // 載入所有插件
    manager.load_all_plugins()?;

    // 列出所有已載入的插件
    println!("\nLoaded Plugins:");
    println!("==============");
    for (name, version, description) in manager.get_all_plugins() {
        println!("{} v{}: {}", name, version, description);
    }
    dbg!(&manager);

    let ret = manager.get_plugin("basic_plugin");
    if let Some(r) = ret {
        // let mut data = HashMap::new();
        // data.insert("action".to_string(), "start".to_string());
        // let event = Event {
        //     name: "event2".to_string(),
        //     data,
        //     priority: 1,
        // };
        println!("{:#?}", r);
        // r.handle_event(&event)?;
    }
    println!("\nUnloading plugins...");
    Ok(())
}
