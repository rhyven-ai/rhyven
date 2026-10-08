# Portable code libraries — 0.6.0

**Apps are complete applications. Pallets are reusable source libraries used to
build them.** Declarative apps still depend on Rhyven's engine. Bricks, mortar
and portable stacks do not need Rhyven to execute their ordinary source code.

| Component | Implementation |
| --- | --- |
| Brick / block | A reusable source function with a typed contract |
| Mortar | A source function adapting values between contracts |
| Portable stack | An ordinary function that calls other reusable functions |
| Pallet | Source files, typed exports and tests in a distinct library format |
| Frame | Project files with separate pallet and app references |
| App | An application with its own capabilities, lifecycle and state |
| App workflow | An engine-managed sequence of calls to installed apps |

The existing app-workflow feature remains available through app compose. Its
operation is still named stack for compatibility, but it is not a portable code
library. Existing app actions are now labeled app_action in discovery, not bricks.

## Reuse without regenerating source

1. Search for the missing operation once. Use backends=["source"] in
   action_match_plan for saved libraries, or include other backends for apps.
2. Inspect the chosen contract. Source is omitted. Read it separately when code
   review, integration or debugging requires it.
3. Export the library once into your project and import its functions normally.
4. Call the imported function or saved CLI export for repeated tasks. Reuse its
   exact version/hash and examples instead of generating another implementation.
5. Create or change a block only when the existing contract does not fit.
   Test the new version and retain it locally. Publishing is a separate,
   explicitly user-requested operation.

The same existing discovery budgets apply: one retry and eight distinct contract
inspections. Pallets use the same three-tool interface through marketplace
functions; they do not add a permanent MCP tool per function.

action_pallet_list lists local saved libraries. action_pallet_describe takes an
exact selector and optional export; without export it returns a short index.
Reuse contract_hash through if_hash for unchanged metadata. Descriptions and
plan shortlists do not return implementation source. Source candidates have
consumer-defined execution permissions: searching/exporting source grants no
permission to execute it. The consuming app or harness selects the backend and
approves its scope.

These mechanisms avoid repeated source generation and description loading.
No model token-saving percentage is claimed.

## Author and save

```sh
rhyven pallet init local/text-tools --dir ./text-tools --language python
rhyven pallet validate ./text-tools
# Review the source before running unsandboxed tests.
rhyven pallet test ./text-tools --allow-host
rhyven --collection project pallet save ./text-tools
rhyven --collection project pallet list
rhyven --collection project pallet describe local/text-tools@0.1.0 --export transform
```

Saving executes no code, grants no execution permission, publishes nothing,
and does not register an app or category. Saved versions are immutable:
change the version when changing source, contracts or tests. Pallets are
SQLite records in the selected scope. Existing collection-scoped records remain readable.

A directory contains pallet.json plus ordinary source files:

```json
{
  "format": "rhyven.pallet/1",
  "name": "local/text-tools",
  "version": "0.1.0",
  "description": "Reusable text transformations",
  "language": "python",
  "files": ["capabilities.py"],
  "exports": {
    "transform": {
      "kind": "brick",
      "description": "Return a supplied value",
      "file": "capabilities.py",
      "symbol": "transform",
      "input": {
        "type": "object",
        "properties": {"value": {"type": "string"}},
        "required": ["value"],
        "additionalProperties": false
      },
      "output": {
        "type": "object",
        "properties": {"value": {"type": "string"}},
        "required": ["value"],
        "additionalProperties": false
      }
    }
  },
  "tests": [
    {"export": "transform", "args": {"value": "sample"},
     "expect": {"value": "sample"}}
  ],
  "dependencies": []
}
```

```python
def transform(args):
    return {"value": args["value"]}
```

Exports can be brick, mortar or stack. They are source symbols; they do not
require app IDs or calls through the engine. A portable stack is ordinary
code composition with normal language semantics, not a generated distributed
workflow. The exported code owns its own control flow.

The JSON contract is metadata. Rhyven validates it during run/test; direct
imports and standalone launchers do not automatically enforce that schema.
Authors should validate inputs themselves where standalone callers need it.

## Export and use independently

```sh
rhyven --collection project pallet export local/text-tools@0.1.0 --dir ./vendor/text_tools
python3 -c 'from vendor.text_tools.capabilities import transform; print(transform({"value":"reuse"}))'
```

Export includes the original source, manifest, version/hash lock and a standalone
JSON launcher for Python/JavaScript. It refuses existing output directories.
It does not copy secrets, installed-app state or the Rhyven runtime.

```sh
echo '{"export":"transform","args":{"value":"reuse"}}' | python3 ./vendor/text_tools/run.py
```

These files work without the Rhyven binary. Interpreter and declared source
dependencies remain necessary. The exporter does not prove arbitrary source
has no undeclared dependencies.

For a quick reviewed operation without exporting manually:

```sh
rhyven --collection project pallet run local/text-tools@0.1.0 transform --args '{"value":"reuse"}' --allow-host
```

run/test execute unsandboxed code with the user's OS access, in temporary
directories. They do not install an app, automatically install dependencies,
or provide persistent pallet state. Test examples are not a safety certificate.
There is deliberately no MCP operation that executes arbitrary saved source
without a reviewed execution path. Agents can use their harness's authorized
shell/code execution, or use a complete installed app.

## Dependencies and languages

Source export is language-neutral: Rust, Go and other projects can include their
normal source/manifests. Their consumers use standard compilers and bindings.
Python/JavaScript have built-in standalone launchers and run/test support;
other languages currently support source validation/export, not automatic builds.

Python exports use .py modules and require Python 3.10+. JavaScript exports use
.mjs ES modules and require Node 20+. Each exported function accepts one JSON
value and returns one JSON value. Keep stdout for the launcher's result; use
stderr for diagnostic output. JavaScript functions may return promises.

The dependencies array is descriptive, not a new package manager or lockfile.
Include the language's real dependency files in files. Prepare the environment
explicitly; run/test with dependencies requires --interpreter /absolute/path/to/python
or node. A compatible existing virtual environment can be used. Rhyven does not
modify a shared environment or install tools behind the user's back.

Limits: 1 MiB per pallet, 128 UTF-8 source files, 32 exports, 8 KiB per export
contract, 64 example tests, and 200 saved versions per collection. Individual
run/test calls have a 30-second subprocess timeout and existing output bounds.
Non-text build artifacts belong in normal language build/distribution tooling or
the existing native/container app backends.

## Build a complete app from a pallet

The document-intake example imports text preparation functions, adds persistent
storage and duplicate handling, and exposes application-level actions.

```sh
rhyven --collection project pallet save examples/pallet-text
rhyven --collection project app bundle examples/document-intake --pallet text=example/text-kit@0.1.0 --out ./document-intake.json
rhyven app validate ./document-intake.json
rhyven app test ./document-intake.json --allow-host
```

The app imports from vendor.text.textkit. Source is bundled into its existing
script package, with pinned library provenance and file hashes. The recipient
installs one complete app, with no separate pallet installation or runtime calls
to a library app. App state still belongs to its collection.

Bundling currently targets Python/JavaScript apps of the same language. It does
not merge dependency locks: declare the app's locked dependencies before bundling
libraries that require them. Use explicit adapters/bindings for other languages.
Ordinary exported source can also be used in applications unrelated to Rhyven.

Frames separate pallets (exact library@version selectors) from apps (app IDs).
They preview and write project files only; neither list authorizes installation.

## Local examples and verification

- examples/pallet-text: Python bricks, mortar and a composed function.
- examples/pallet-text-js: equivalent JavaScript functions.
- examples/document-intake: complete app importing the Python pallet.
- examples/document-intake/frame.json: app project scaffold with a pallet reference.

cargo test -p agent-market-core --test pallets checks independent execution,
reuse without app registration, isolated app bundling, persistence across agents,
backup/restore, contract caching, immutable versions and source tampering.

Pallets are separate marketplace source libraries. Local creation, export and
reuse do not authorize public posting. Only publish when the user requests it.


## Workspace and global libraries

Bind a project explicitly with `--project`. This is independent of the app
collection; connecting an agent preserves the absolute project path in its MCP
launch arguments. Without a project binding, existing collection-local saves
remain supported for compatibility. Workspace downloads require a binding.

```sh
rhyven --project "$PWD" pallet save ./text-tools --scope workspace
rhyven --project "$PWD" pallet list
rhyven --project "$PWD" pallet promote workspace::local/text-tools@0.1.0
rhyven pallet save ./text-tools --scope global
rhyven --project "$PWD" connect --client codex
```

- Workspace source is stored in `<project>/.rhyven/state.sqlite3` alongside the
  workspace's legacy state, independently of the selected collection.
- Global source is stored in the `global` collection database under the selected
  Rhyven home. It is visible to all collections using that home, not other OS users
  or agents using another Rhyven home.
- Discovery searches workspace, existing collection and global libraries in one
  bounded search and labels the scope. Workspace candidates are indexed first;
  relevance still determines the shortlist.
- If the same name/version has different contents in two scopes, unqualified
  resolution fails. Use `workspace::name@version` or `global::name@version` to
  select deliberately. Source hashes remain pinned when bundled into apps.
- Global visibility grants no execution permission and shares no app data.
- Collection backups include that collection's pallets only. Back up workspace
  and global libraries separately; a project collection backup does not include
  every global library. Exported source can also be versioned with project code.

## Marketplace pallets

Open the TUI and press **p** for Pallets. Select a listing, then **w** for workspace
or **g** for global. Review its source repository, version, language, license,
hash and trust label; **y** confirms the source download. No code is executed.
The main registry includes `rhyven/text-kit`, an Apache-2.0 example for whitespace
normalization, ASCII URL slugs and document-title preparation.

```sh
rhyven registry-sync rhyven-ai/registry --anonymous
rhyven pallet search text
# After user approval of this source package:
rhyven --project "$PWD" pallet download rhyven/text-kit@0.1.0 --scope workspace --accept-source
```

Agents use the existing three MCP tools:

1. `rhyven_call` with category `rhyven/marketplace`, function
   `action_pallet_search`, args `{"query":"text"}`.
2. `action_prepare_pallet` with exact `selector` and `scope` (`workspace` or
   `global`). The response pins the listing and destination for human review.
3. `action_apply` with its `request_id`, through the existing MCP host elicitation
   or human terminal approval. The agent cannot approve its own request.
4. `action_pallet_list` and `action_pallet_describe` inspect saved contracts.

`--accept-source` records operator consent; an agent must not supply it without
explicit user authorization. Running downloaded source requires separate review.

## Preparing a pallet listing

```sh
rhyven pallet package ./text-tools --out text-tools-0.1.0.json
sha256sum text-tools-0.1.0.json
```

Packaging writes a new source-only asset and never publishes it. When the user
requests publication, upload that file to a GitHub release and submit a registry
PR. Keep app listings in `index.json`. Add a separate `pallets.json` with
`{"format":1,"pallets":[...]}`; each pallet entry has this shape:

```json
{
  "name": "example/text-tools",
  "version": "0.1.0",
  "description": "Text normalization functions",
  "language": "python",
  "license": "Apache-2.0",
  "repository": "example/text-tools",
  "asset_id": 123456,
  "sha256": "REPLACE_WITH_RELEASE_FILE_SHA256"
}
```

Keeping the files separate lets older runtimes read the app index. Version 0.6.0
fetches both files and validates them together; only a missing pallet file is
optional, while malformed files and access failures stop the refresh.

The publisher namespace must map to the GitHub repository owner in `publishers`.
Pallet listings require an accepted open-source license. The package must match
all listing metadata. `registry-check` downloads and validates source and hashes,
but never executes publisher tests. Existing entries are immutable; corrections
require a new version. App and pallet listings remain separate types.
