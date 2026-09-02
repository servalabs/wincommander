use wincmd_shared::svc::{ApplyMachineSettingRequest, MachineSettingObserved};

#[tauri::command]
pub async fn apply_machine_setting(
    request: ApplyMachineSettingRequest,
) -> Result<MachineSettingObserved, String> {
    crate::svc_client::apply_machine_setting(request).await
}
