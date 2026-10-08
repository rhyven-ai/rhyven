"""Portable text helpers. No Rhyven imports, storage or environment assumptions."""
import re


def normalize(args):
    return {"text": " ".join(args["text"].split())}


def slug(args):
    return {"slug": re.sub(r"[^a-z0-9]+", "-", args["text"].lower()).strip("-")}


def rename_title(args):
    """Mortar: adapt a title field into the text contract."""
    return {"text": args["title"]}


def prepare_document(args):
    """A portable stack is ordinary code that composes reusable functions."""
    adapted = rename_title(args)
    cleaned = normalize(adapted)
    return {"title": cleaned["text"], "slug": slug(cleaned)["slug"]}
