//! cargo run -p zephium-ipc --example export_work -- <output.ts>
use zephium_ipc::work::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("supply the output TypeScript path")?;
    let types = specta::Types::default()
        .register::<WorkCallV1>()
        .register::<WorkChangedV1>()
        .register::<WorkEnvironmentChangedV1>()
        .register::<WorkProjectionV1>()
        .register::<WorkCommandV1>()
        .register::<WorkAuthoringCommandV1>()
        .register::<WorkQueryV1>()
        .register::<WorkResponseV1>()
        .register::<WorkPlanRequestV1>()
        .register::<WorkPlanningResponseV1>()
        .register::<WorkApprovalRequestV1>()
        .register::<WorkStartRequestV1>()
        .register::<WorkOperationV1>()
        .register::<WorkOperationResponseV1>()
        .register::<WorkSignalV1>()
        .register::<WorkActivityResponseV1>();
    let mut output =
        specta_typescript::Typescript::default().export(&types, specta_serde::Format)?;
    output.push_str("\nexport type WorkProjectionV1 = WorkRuntimeProjection;\n");
    let output = output
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(path, output)?;
    Ok(())
}
