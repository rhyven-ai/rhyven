export function normalize(args) {
  return {text: args.text.trim().split(/\s+/).join(' ')};
}
export function slug(args) {
  return {slug: args.text.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')};
}
export function renameTitle(args) {
  return {text: args.title};
}
export function prepareDocument(args) {
  const clean = normalize(renameTitle(args));
  return {title: clean.text, slug: slug(clean).slug};
}
