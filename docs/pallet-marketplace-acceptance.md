# Pallet marketplace acceptance — local 0.6.0

Verified on Linux on 2026-10-08 using the local release binary.

## Public test fixture

Only [rhyven-ai/pallet-text-test](https://github.com/rhyven-ai/pallet-text-test)
was published, with explicit user approval. The production registry, website and
0.6.0 runtime were not published. The test repository contains source, an
Apache-2.0 license and a separate registry fixture with no app listings.

- Pallet: `rhyven-test/text-kit@0.1.0`
- Release: https://github.com/rhyven-ai/pallet-text-test/releases/tag/v0.1.0
- Release asset: `text-kit-0.1.0.json`, 15,733 bytes
- SHA-256: `afa18d564761ae18b775fa30f371618fd1a9a7020b7cac785233d110ab17e414`
- Exports: normalize whitespace, create an ASCII URL slug, adapt a title field,
  and compose those functions into document preparation.
- Dependencies: Python standard library only.

Input `{"title":"  Customer   Release Notes!  "}` returned
`{"title":"Customer Release Notes!","slug":"customer-release-notes"}`.
Slug generation is ASCII-only, does not transliterate Unicode and does not
guarantee uniqueness. These limits are documented in the public repository.

## Results

All checks passed:

1. The four declared examples passed before publishing.
2. A fresh temporary Rhyven home synced the real GitHub fixture anonymously.
3. The three-tool MCP interface found and described the listing.
4. Workspace and global source downloads used real GitHub release assets,
   verified the pinned hash, and saved to the reviewed destinations.
5. The MCP server emitted host elicitation. A minimal test client relayed the
   user's explicit approval; this was not a live Codex or Cline session.
6. Retrying a completed approval returned the same receipt without downloading
   again.
7. A second agent/collection could reuse the global library. Before global
   download, that collection could not see the workspace-only library.
8. Exported functions ran using ordinary Python imports, without Rhyven.
9. Downloading libraries added no installed app or MCP category.
10. An actual TUI running in a pseudo-terminal opened the Pallets view, showed
    source details and confirmation, and downloaded to workspace and global
    scopes. The terminal check reconstructed incremental screen updates.
11. Unit tests rejected downloads before consent, tampered asset bytes and
    changed listings after approval. Reconnecting without a project did not
    redirect an already approved destination.

The live tests used temporary homes on this Linux host, not a new VM or another
operating system. Only the public test fixture remains; temporary app state was
removed.

## Repeat the MCP acceptance test

After reviewing the test source and authorizing temporary downloads and execution:

```sh
python3 qa/pallet_market_check.py target/release/rhyven --approve-test-source
```

This requires network access to GitHub and a compatible Python interpreter. It
checks the exact hash above, uses disposable homes and does not modify the public
registry. The consent flag belongs to the operator; agents must not invent consent.

The runtime still needs a coordinated release before the public website can
describe this as available in its current download.
