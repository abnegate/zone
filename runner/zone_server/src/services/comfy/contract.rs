use abnegate_comfy::train::Contract;

pub const TRAIN_LORA_NODE: &str = "ZoneTrainLoRA";
pub const CLEANUP_TRAINING_RUN_NODE: &str = "ZoneCleanupTrainingRun";
pub const LOAD_TRAIN_DATASET_NODE: &str = "ZoneLoadTrainDataset";
pub const PROBE_LOSS_NODE: &str = "ZoneProbeLoss";
pub const STAGE_TRAINING_ARTIFACT_NODE: &str = "ZoneStageTrainingArtifact";
pub const IPADAPTER_FACE_NODE: &str = "ZoneIPAdapterFace";
pub const FOLDER_PREFIX: &str = "zone-train-";
pub const ARTIFACT_PREFIX: &str = "zone-lora-";
pub const PROBE_PREFIX: &str = "zone-probe-";
pub const ENVIRONMENT_PREFIX: &str = "ZONE_TRAIN";
pub const INPUT_ENVIRONMENT_PREFIX: &str = "ZONE_COMFY";
pub const SIDECAR_SUFFIX: &str = ".zone.json";
pub const PUBLICATION_DIRECTORY: &str = ".zone-publish";
pub const PROGRESS_SUFFIX: &str = "-progress.json";
pub const HOST_DIRECTORY: &str = ".zone-train";

/// The node, namespace and file names zone's custom nodes, Python trainers
/// and host trainer are written against.
pub fn contract() -> Contract {
    let mut contract = Contract::default();
    contract.train_lora_node = TRAIN_LORA_NODE.to_string();
    contract.cleanup_training_run_node = CLEANUP_TRAINING_RUN_NODE.to_string();
    contract.load_train_dataset_node = LOAD_TRAIN_DATASET_NODE.to_string();
    contract.probe_loss_node = PROBE_LOSS_NODE.to_string();
    contract.stage_training_artifact_node = STAGE_TRAINING_ARTIFACT_NODE.to_string();
    contract.ipadapter_face_node = IPADAPTER_FACE_NODE.to_string();
    contract.folder_prefix = FOLDER_PREFIX.to_string();
    contract.artifact_prefix = ARTIFACT_PREFIX.to_string();
    contract.probe_prefix = PROBE_PREFIX.to_string();
    contract.environment_prefix = ENVIRONMENT_PREFIX.to_string();
    contract.input_environment_prefix = INPUT_ENVIRONMENT_PREFIX.to_string();
    contract.sidecar_suffix = SIDECAR_SUFFIX.to_string();
    contract.publication_directory = PUBLICATION_DIRECTORY.to_string();
    contract.progress_suffix = PROGRESS_SUFFIX.to_string();
    contract.host_directory = HOST_DIRECTORY.to_string();
    contract
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones_contract_is_one_the_crate_accepts() {
        assert_eq!(contract().validate(), Ok(()));
    }

    #[test]
    fn the_contract_carries_the_names_zones_nodes_and_trainers_read() {
        let zone = contract();
        assert_eq!(zone.train_lora_node, "ZoneTrainLoRA");
        assert_eq!(zone.cleanup_training_run_node, "ZoneCleanupTrainingRun");
        assert_eq!(zone.load_train_dataset_node, "ZoneLoadTrainDataset");
        assert_eq!(zone.probe_loss_node, "ZoneProbeLoss");
        assert_eq!(
            zone.stage_training_artifact_node,
            "ZoneStageTrainingArtifact"
        );
        assert_eq!(zone.ipadapter_face_node, "ZoneIPAdapterFace");
        assert_eq!(zone.folder_prefix, "zone-train-");
        assert_eq!(zone.artifact_prefix, "zone-lora-");
        assert_eq!(zone.probe_prefix, "zone-probe-");
        assert_eq!(zone.environment_prefix, "ZONE_TRAIN");
        assert_eq!(zone.input_environment_prefix, "ZONE_COMFY");
        assert_eq!(zone.sidecar_suffix, ".zone.json");
        assert_eq!(zone.publication_directory, ".zone-publish");
        assert_eq!(zone.progress_suffix, "-progress.json");
        assert_eq!(zone.host_directory, ".zone-train");
    }

    #[test]
    fn no_name_is_the_crates_neutral_default() {
        let zone = contract();
        let neutral = Contract::default();
        assert_ne!(zone.train_lora_node, neutral.train_lora_node);
        assert_ne!(zone.ipadapter_face_node, neutral.ipadapter_face_node);
        assert_ne!(zone.folder_prefix, neutral.folder_prefix);
        assert_ne!(zone.environment_prefix, neutral.environment_prefix);
        assert_ne!(zone.sidecar_suffix, neutral.sidecar_suffix);
        assert_ne!(zone.host_directory, neutral.host_directory);
    }
}
