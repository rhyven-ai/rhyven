# Website security audit — 2026-09-27

Scope: the public Rhyven website, its browser code, static deployment boundary,
download bootstrap and deployed release integrity. This is a first-party review
with bounded live checks and adversarial browser tests, not an independent
penetration test. It does not re-audit the Rust runtime, app containers, registrar
account or Cloudflare's infrastructure.

## Findings

No critical or high-severity website issue was identified in the tested scope.
Two lower-severity issues were reproduced and corrected:

1. **Catalog values entered copied shell commands without validation.** A
   synthetic registry string containing shell syntax appeared in installation
   instructions. Exploitation would require control of catalog metadata and a
   visitor pasting the command; no such data was found in the deployed catalog.
   App IDs, names, execution types and registry paths are now checked before any
   catalog entries are rendered. Tests reject 20 command injection variants,
   including substitutions, separators, control characters and trailing newlines.
   Text fields remain HTML-escaped. App names that match JavaScript prototype
   property names now use the normal fallback presentation.
2. **Mutable JavaScript was cached for four hours by the domain proxy.** This
   could delay a security correction for returning visitors. Production staging
   now generates content-hashed JavaScript filenames and rewrites imports in
   dependency order. A dependency edit changes every affected module URL. Mutable
   pages and data request `no-store`. Immutable script and release paths have
   separate cache rules, avoiding the previous combination of `no-cache` and
   `immutable`. `no-transform` remains in place to prevent proxy script injection.

The website's documentation and downloadable authoring skills now advertise
Linux only, as requested. Existing signed release assets are not overwritten.

## Live checks

- Both custom domains serve the expected site over certificate-verified HTTPS;
  HTTP redirects to HTTPS. TLS 1.2 and 1.3 succeeded; TLS 1.0 and 1.1 were rejected
  with protocol-version alerts. The observed certificate expires December 26,
  2026; renewal is managed by the hosting provider.
- All 33 public assets matched the reviewed deployment bytes before this update,
  following the host's canonical redirects for HTML files.
- The live release manifest's signature verified against the maintainer's local
  public key, independently of the downloaded key. All ten signed assets,
  including every binary, installer and notices, matched their signed hashes.
- Nineteen bounded exposure probes on each custom domain found no accessible
  Git metadata, environment files, signing key, local state, source archive,
  preview server, source map or internal hosting configuration. Invalid download
  paths returned error responses instead of the homepage.
- POST, PUT, DELETE, TRACE and OPTIONS against a nonexistent audit path returned
  405. The website has no application backend, authentication session, payment
  handler or connection to a visitor's runtime.
- CSP restricts scripts and connections to the same origin, disables framing,
  objects, base changes and form submissions, and permits neither inline scripts
  nor eval. HSTS, MIME sniffing protection, frame protection, referrer policy and
  browser capability restrictions are present.
- Public static assets allow cross-origin reads without credentials. This is
  expected for public downloads and is not a customer-data access boundary.

## Source and browser checks

- Gitleaks 8.30.1, verified against the pinned upstream archive checksum, found no
  secrets in the staged static text assets or six reviewed public Git commits.
  Binary authenticity was checked cryptographically; the secret scan does not
  claim exhaustive analysis of compiled machine code.
- Synthetic catalog descriptions, permissions, objects, action text, search input
  and Markdown content render as text rather than executable markup.
- Skill navigation uses known IDs and cannot turn a fragment into an arbitrary
  file request. Clipboard copying preserves valid installation instructions.
- Browser checks observed no cookies, local/session storage or third-party
  resource requests. Cloudflare can still retain hosting/security request logs;
  their account-level retention settings were not audited.
- Functional and accessibility acceptance passed all five pages, seven app cards,
  thirteen docs topics, five skills and the usage rule, at three viewport widths.
- Two staging regression tests verify dependency-aware URL changes and separate
  mutable/immutable cache policies.

Raw evidence is retained locally in ignored `dist/website-security-audit/`.
Repeat the browser checks with `qa/website_security_check.cjs` and
`qa/website_check.cjs`; staging tests are `qa/test_website_staging.py`.

## Remaining owner actions and boundaries

1. **DNSSEC is not active end to end.** Public DNS returned no parent DS record.
   Enable DNSSEC in Cloudflare, then add its exact DS values at Network Solutions
   and verify the completed chain. This is coordinated domain hardening, not a
   website-code fix. [Cloudflare DNSSEC instructions](https://developers.cloudflare.com/dns/dnssec/)
2. **Account controls were not verified.** Confirm MFA/passkeys on Cloudflare,
   Network Solutions and GitHub, registrar transfer lock, recovery access and an
   offline backup of the release signing key. No credentials or private keys were
   printed, uploaded or rotated during the audit.
3. **DMARC is monitoring only (`p=none`).** Review legitimate mail senders before
   moving to enforcement. This affects email impersonation, not website TLS.
4. **The initial installer still trusts HTTPS and the hosting account.** Signatures
   protect downloads relative to the trusted installer/key, but replacing both
   the bootstrap script and its embedded key defeats that first-download trust.
   For independent verification, retain a trusted key or compare its fingerprint
   through a separately trusted channel. SHA-256 of the current public PEM bytes:
   `7a576279692e38f234a8a4d65ada33824abdab2b6c6b37086eae39b2ce6540f9`.

CAA was absent in the public lookup. It is optional additional certificate
issuance control, not evidence of a broken TLS configuration. Do not restrict
issuers without accounting for the host's automatic certificate renewal.
