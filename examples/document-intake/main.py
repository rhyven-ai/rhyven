"""Complete app: normalize incoming document titles, persist and list documents."""
import json
import os
from pathlib import Path
import sys
from vendor.text.textkit import prepare_document

request = json.loads(sys.stdin.readline())
store = Path(os.environ["RHYVEN_DATA_DIR"]) / "documents.json"
documents = json.loads(store.read_text()) if store.exists() else []
if request["function"] == "action_add_document":
    document = prepare_document(request["args"])
    if any(d["slug"] == document["slug"] for d in documents):
        raise ValueError("A document with this slug already exists")
    documents.append(document)
    temporary = store.with_suffix(".tmp")
    temporary.write_text(json.dumps(documents))
    temporary.replace(store)
    result = document
elif request["function"] == "action_list_documents":
    result = {"documents": documents}
else:
    raise ValueError("Unknown action")
print(json.dumps({"result": result}))
