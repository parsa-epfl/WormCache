use crate::checkpoint::helpers::RPTPerCoreHelper;
use serde_json::json;

pub fn process_ibsp(checkpoint_folder: &String, output_folder: &String) {
    let (is_rkyv, rpt_checkpoints) =
        crate::checkpoint::detect_checkpoint_files(checkpoint_folder, "rpt");
    if rpt_checkpoints.is_empty() {
        println!("No RPT checkpoint files found; skipping IBSP conversion.");
        return;
    }

    let mut rpt_tables = Vec::new();
    for file_name in &rpt_checkpoints {
        let path = format!("{}/{}", checkpoint_folder, file_name);
        let bytes = crate::util::read_compressed(&path);
        let worker: Vec<RPTPerCoreHelper> = if is_rkyv {
            rkyv::from_bytes::<Vec<RPTPerCoreHelper>, rkyv::rancor::Error>(&bytes).unwrap()
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        rpt_tables.extend(worker);
    }

    for (core_id, table) in rpt_tables.into_iter().enumerate() {
        let file_name = format!("{}/{:03}-rpt.json", output_folder, core_id);
        let file = std::fs::File::create(&file_name).unwrap();
        serde_json::to_writer(file, &json!({ "rpt": table })).unwrap();
        println!("RPT for core {} exported to {}", core_id, file_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint::helpers::{RPTEntryHelper, RPTSetHelper};

    #[test]
    fn converts_checkpoint_tables_to_per_core_json() {
        let checkpoint_dir = tempfile::tempdir().unwrap();
        let output_dir = tempfile::tempdir().unwrap();
        let helper = vec![RPTPerCoreHelper {
            sets: vec![RPTSetHelper {
                entries: vec![RPTEntryHelper {
                    tag: 0x12,
                    last_block: 0x34,
                    last_stride: -2,
                    ts: 56,
                }],
            }],
        }];
        let bytes = serde_json::to_vec(&helper).unwrap();
        crate::util::write_compressed(
            &format!(
                "{}/rpt-0-worker-0.json.zstd",
                checkpoint_dir.path().display()
            ),
            &bytes,
        );

        process_ibsp(
            &checkpoint_dir.path().display().to_string(),
            &output_dir.path().display().to_string(),
        );

        let output: serde_json::Value = serde_json::from_reader(
            std::fs::File::open(output_dir.path().join("000-rpt.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(output["rpt"]["sets"][0]["entries"][0]["last_stride"], -2);
    }
}
