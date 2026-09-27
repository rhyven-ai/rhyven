//! Transport-independent tool descriptions and routing; the runtime enforces all rules.
use crate::{http::HttpClient, Error, Result, Runtime};
use serde_json::{json, Value};

pub struct Tool {
    pub definition: Value,
    operation: String,
    fixed: Value,
}
pub struct AgentSession {
    backend: Backend,
    pub tools: Vec<Tool>,
}
enum Backend {
    Local(Runtime),
    Shared(HttpClient),
}
impl Backend {
    fn describe(&self, app: &str) -> Result<Value> {
        match self {
            Self::Local(r) => r.describe(app),
            Self::Shared(r) => r.describe(app),
        }
    }
    fn call(&self, operation: &str, args: Value) -> Result<Value> {
        match self {
            Self::Local(r) => r.call(operation, args),
            Self::Shared(r) => r.call(operation, args),
        }
    }
}
fn string() -> Value {
    json!({"type":"string"})
}
fn free() -> Value {
    json!({"type":"object","additionalProperties":true})
}
fn tool(
    name: &str,
    description: &str,
    properties: Value,
    required: &[&str],
    operation: &str,
    fixed: Value,
) -> Tool {
    Tool {
        definition: json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}}),
        operation: operation.into(),
        fixed,
    }
}
fn contract_tools(p: &Value) -> Vec<Tool> {
    let app = p["name"].as_str().unwrap();
    let mut tools = vec![];
    tools.push(tool(
        "describe_app",
        "Read this app's contract, actions, hosting and guide",
        json!({}),
        &[],
        "describe_app",
        json!({"app":app}),
    ));
    for (name, object) in p["objects"].as_object().unwrap() {
        let fixed = json!({"app":app,"object":name});
        if p["platform"] != true {
            tools.push(tool(
                &format!("object_{name}_create"),
                "Create a validated record",
                json!({"data":object["schema"],"request_id":string()}),
                &["data"],
                "create",
                fixed.clone(),
            ));
        }
        tools.push(tool(
            &format!("object_{name}_get"),
            "Read record with revision",
            json!({"id":string()}),
            &["id"],
            "get",
            fixed.clone(),
        ));
        if p["platform"] != true || name == "listing" {
            tools.push(tool(&format!("object_{name}_query"),"Query equality filters; use limit/offset for pages",json!({"filters":free(),"limit":{"type":"integer","minimum":0,"maximum":1000},"offset":{"type":"integer","minimum":0}}),&[],"query",fixed.clone()));
        }
        if object["immutable"] != true {
            let mut partial = object["schema"].clone();
            partial["required"] = json!([]);
            for s in partial["properties"].as_object_mut().unwrap().values_mut() {
                s.as_object_mut().unwrap().remove("default");
            }
            tools.push(tool(&format!("object_{name}_update"),"Patch with revision guard; protected fields require actions",json!({"id":string(),"patch":partial,"expected_revision":{"type":"integer","minimum":1},"request_id":string()}),&["id","patch","expected_revision"],"update",fixed));
        }
    }
    if p["platform"] != true && p["hosting"]["mode"] == "local" {
        for tool in &mut tools {
            if tool.operation == "query" {
                let object = &p["objects"][tool.fixed["object"].as_str().unwrap()];
                tool.definition["inputSchema"]["properties"]
                    .as_object_mut()
                    .unwrap()
                    .extend(
                        crate::query::properties(object)
                            .as_object()
                            .unwrap()
                            .clone(),
                    );
                tool.definition["description"] = json!("Query equality filters, typed where comparisons and metadata timestamps (AND). Optional search/current_only are available when declared by the object. order_by supports data fields and $created_at/$updated_at before limit/offset pagination. Missing fields never match where and sort last.");
            }
        }
    }
    for (name, action) in p["actions"].as_object().unwrap() {
        tools.push(tool(
            &format!("action_{name}"),
            action["description"]
                .as_str()
                .unwrap_or("Execute declared action"),
            json!({"args":action["input"],"request_id":string()}),
            &["args"],
            "execute",
            json!({"app":app,"action":name}),
        ));
    }
    tools
}

impl AgentSession {
    /// Empty selection means the stable universal interface; one app means standalone mode.
    pub fn new(runtime: Runtime, apps: &[String]) -> Result<Self> {
        Self::with_backend(Backend::Local(runtime), apps)
    }
    /// The shared server owns the runtime. Token is used only for this process's HTTP requests.
    pub fn remote(endpoint: &str, token: String, apps: &[String]) -> Result<Self> {
        Self::with_backend(Backend::Shared(HttpClient::new(endpoint, token)?), apps)
    }
    fn with_backend(backend: Backend, apps: &[String]) -> Result<Self> {
        if apps.len() > 1 {
            return Err(Error::new(
                "validation",
                "Standalone mode takes exactly one app",
            ));
        }
        let mut tools = vec![];
        if let Some(app) = apps.first() {
            crate::error::ensure(
                app != crate::marketplace::APP,
                "permission",
                "Marketplace is a universal platform capability, not a standalone app",
            )?;
            let p = backend.describe(app)?;
            tools = contract_tools(&p);
        } else {
            tools.push(tool(
                "rhyven_categories",
                "Discover installed app categories and the platform marketplace",
                json!({}),
                &[],
                "rhyven_categories",
                json!({}),
            ));
            tools.push(tool("rhyven_describe", "Read a category's machine-readable function manifest, schemas and Markdown guidance before calling it", json!({"category":string()}), &["category"], "rhyven_describe", json!({})));
            tools.push(tool("rhyven_call", "Call a discovered function with arguments validated against its manifest. Marketplace downloads require user approval", json!({"category":string(),"function":string(),"args":free()}), &["category","function","args"], "rhyven_call", json!({})));
        }
        Ok(Self { backend, tools })
    }
    /// Host-only consent, never exposed as a tool or schema action.
    pub fn approval_review(&self, id: &str) -> Result<Value> {
        match &self.backend {
            Backend::Local(r) => crate::marketplace::review(r, id),
            Backend::Shared(c) => Ok(c.call(
                "get",
                json!({"app":crate::marketplace::APP,"object":"request","id":id}),
            )?["data"]
                .clone()),
        }
    }
    pub fn approve(&self, id: &str, digest: &str, accept: bool) -> Result<Value> {
        match &self.backend {
            Backend::Local(r) => crate::marketplace::decide(r, id, digest, accept),
            Backend::Shared(c) => c.approve(id, digest, accept),
        }
    }
    pub fn definitions(&self) -> Vec<Value> {
        self.tools.iter().map(|t| t.definition.clone()).collect()
    }
    pub fn call(&mut self, name: &str, mut args: Value) -> Result<Value> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.definition["name"] == name)
            .ok_or_else(|| Error::new("unknown_tool", name))?;
        let map = args
            .as_object_mut()
            .ok_or_else(|| Error::new("validation", "Arguments must be object"))?;
        let props = tool.definition["inputSchema"]["properties"]
            .as_object()
            .unwrap();
        for key in map.keys() {
            if !props.contains_key(key) {
                return Err(Error::new("validation", format!("Unknown argument: {key}")));
            }
        }
        for required in tool.definition["inputSchema"]["required"]
            .as_array()
            .unwrap()
        {
            if !map.contains_key(required.as_str().unwrap()) {
                return Err(Error::new("validation", format!("Missing {required}")));
            }
        }
        for (key, value) in tool.fixed.as_object().unwrap() {
            map.insert(key.clone(), value.clone());
        }
        self.backend.call(&tool.operation, args)
    }
}

/// Build the same function manifest for local MCP, REST and remote MCP.
pub fn manifest(package: &Value) -> Value {
    let functions:Vec<_>=contract_tools(package).into_iter().filter(|t|t.operation!="describe_app").map(|t| {
        let mut input=t.definition["inputSchema"].clone();
        if t.operation=="execute" {
            input=package["actions"][t.fixed["action"].as_str().unwrap()]["input"].clone();
            if package["platform"] != true && input["properties"].get("request_id").is_none() { input["properties"]["request_id"]=string(); }
        } else if t.operation=="query" {
            let mut filters=package["objects"][t.fixed["object"].as_str().unwrap()]["schema"].clone();
            filters["required"]=json!([]);
            for field in filters["properties"].as_object_mut().unwrap().values_mut() {field.as_object_mut().unwrap().remove("default");}
            if package["platform"]==true { filters=json!({"type":"object","properties":{"search":string(),"name":string(),"installed":{"type":"boolean"},"update_available":{"type":"boolean"}},"additionalProperties":false}); }
            input["properties"]["filters"]=filters;
        }
        let output = if t.operation=="execute" {package["actions"][t.fixed["action"].as_str().unwrap()].get("output").cloned().unwrap_or(json!({"type":"object"}))} else {json!({"type":"object"})};
        json!({"name":t.definition["name"],"description":t.definition["description"],"inputSchema":input,"outputSchema":output})
    }).collect();
    json!({"category":package["name"],"version":package["version"],"description":package["description"],"functions":functions,"guidance_markdown":package["guide"],"contract":package})
}
pub fn invoke(runtime: &Runtime, args: Value) -> Result<Value> {
    crate::catalog::keys(&args, &["category", "function", "args"])?;
    let category = args["category"]
        .as_str()
        .ok_or_else(|| Error::new("validation", "category must be a string"))?;
    let package = runtime.describe(category)?;
    let function = args["function"]
        .as_str()
        .ok_or_else(|| Error::new("validation", "function must be a string"))?;
    let manifest = manifest(&package);
    let definition = manifest["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == function)
        .ok_or_else(|| Error::new("not_found", "Function is not declared by this category"))?;
    let mut input = crate::schema::validate(
        args.get("args").cloned().unwrap_or(json!({})),
        &definition["inputSchema"],
    )?;
    let tool = contract_tools(&package)
        .into_iter()
        .find(|t| t.definition["name"] == function)
        .unwrap();
    let mut actual = tool.fixed;
    if tool.operation == "execute" {
        let action = actual["action"].as_str().unwrap();
        if package["actions"][action]["input"]["properties"]
            .get("request_id")
            .is_none()
        {
            if let Some(id) = input.as_object_mut().unwrap().remove("request_id") {
                actual["request_id"] = id;
            }
        }
        actual["args"] = input;
    } else {
        actual
            .as_object_mut()
            .unwrap()
            .extend(input.as_object().unwrap().clone());
    }
    runtime.call(&tool.operation, actual)
}
