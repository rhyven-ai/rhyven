//! Read-only host prerequisites. Actual installation still validates packages and images.
use serde_json::{json, Value};
pub fn check(package: &Value) -> Value {
    let driver = crate::execution::driver(package);
    if package["hosting"].as_str().is_some_and(|s| s != "local")
        || package["hosting"]["mode"]
            .as_str()
            .is_some_and(|s| s != "local")
    {
        return json!({"status":"unchecked","summary":"Remote endpoint access required","remedy":"Configure the declared endpoint credentials","scope":"Endpoint connectivity and authorization have not been tested"});
    }
    match driver {
        "native" => match crate::native::requirements(package) {
            Ok(info) => {
                json!({"status":"ready","summary":"Matching native artifact","tools":info,"scope":"Dynamic library availability and execution have not been tested"})
            }
            Err(error) => {
                json!({"status":"needs_setup","summary":"Native artifact unavailable","error":error,"remedy":"Obtain a matching publisher artifact or use a container"})
            }
        },
        "script" => match crate::script::requirements(package) {
            Ok(tools) => {
                json!({"status":"ready","summary":"Host script prerequisites ready","tools":tools,"scope":"Dependency installation and app-specific external tools are checked separately; host execution is unsandboxed"})
            }
            Err(error) => {
                json!({"status":"needs_setup","summary":"Script prerequisites unavailable","error":error,"remedy":"Install the required Python/Node version and Python venv/ensurepip or npm as applicable; check PATH","scope":"No dependencies installed or app code executed"})
            }
        },
        "container" => {
            let report = crate::container::doctor();
            json!({"status":if report["container"]["ready"] == true {"ready"} else {"needs_setup"},"summary":if report["container"]["ready"] == true {"Docker host ready"} else {"Docker unavailable or incompatible"},"diagnosis":report["container"],"remedy":"Run rhyven doctor; install/start a compatible local Docker engine if required","scope":"Image access, architecture and application health are checked during installation; no image pulled"})
        }
        _ => {
            json!({"status":"ready","summary":"Ready — no additional runtime required","scope":"Declarative app runs in the Rhyven engine"})
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_and_remote_requirements_do_not_claim_remote_readiness() {
        assert_eq!(
            check(&json!({"hosting":{"mode":"local"}}))["status"],
            "ready"
        );
        assert_eq!(
            check(&json!({"hosting":"self-hosted"}))["status"],
            "unchecked"
        );
    }
}
