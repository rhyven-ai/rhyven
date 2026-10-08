# Document intake application

This complete app imports the portable text library and adds persisted documents,
duplicate checking and application actions. The library is bundled as source;
recipients install one app, not an app per function.

Save examples/pallet-text, then run app bundle on this directory with
--pallet text=example/text-kit@0.1.0. Validate/test the resulting package before
reviewed installation. See docs/portable-pallets.md for complete commands.

frame.json previews/creates these project files and references the source pallet.
It does not install or execute dependencies.
