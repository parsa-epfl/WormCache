use rustc_hash::FxHashMap;

mod normal;
mod warm;

pub fn chronic_behavior_init(options: &FxHashMap<String, String>) {
    let normal = "normal".to_string();
    let mode = options.get("mode").unwrap_or(&normal);

    // - mode=normal|communication|warm
    // - quit_threshold_ns=N
    // - init_threshold=N
    // - interval=N
    // - count=N
    // - prefix="name"
    // - init_index=N

    if mode == "warm" {
        println!("Periodical snapshot (warm) is enabled.");

        let init_threshold = options
            .get("init_threshold")
            .map(|x| x.parse::<u64>().unwrap())
            .unwrap();

        let interval = options
            .get("interval")
            .map(|x| x.parse::<u64>().unwrap())
            .unwrap();

        let count = options
            .get("count")
            .map(|x| x.parse::<u64>().unwrap())
            .unwrap();

        let prefix = options
            .get("prefix")
            .unwrap_or(&"snapshot".to_string())
            .clone();

        let init_index = options
            .get("init_index")
            .map(|x| x.parse::<u64>().unwrap())
            .unwrap_or(0);

        let no_qemu_snapshot = options
            .get("no_qemu_snapshot")
            .map(|x| x.parse::<bool>().unwrap())
            .unwrap_or(false);

        let raw_ckpt_fmt = options
            .get("raw_ckpt_fmt")
            .map(|x| x.parse::<bool>().unwrap())
            .unwrap_or(false);

        unsafe {
            warm::init(
                init_threshold,
                interval,
                count,
                prefix,
                init_index,
                no_qemu_snapshot,
                raw_ckpt_fmt,
            );
        }
    } else if let Some(threshold_str) = options.get("quit_threshold_ns") {
        let threshold = threshold_str.parse::<u64>().unwrap();
        println!("Quit threshold enabled: {} cycles.", threshold);
        unsafe {
            normal::init(threshold);
        }
    }
}

pub fn on_loading_snapshot(snapshot_name: &str) {
    warm::on_load_snapshot(snapshot_name);
}

pub fn on_finish_loading_snapshot() {}
