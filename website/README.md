# Zone website

The public site: home, pricing, licensing, contact, the post-checkout thank-you page and a 404 page. It is plain HTML and CSS, rendered by a small Bun script that fills shared values into the templates. There is no client-side JavaScript, no cookies and no tracking.

| Path | What |
| --- | --- |
| `config.ts` | Every value that may change: the Community member limit, each paid plan's monthly and yearly seat price and Polar checkout link, contact address and form endpoint, domain, repository |
| `templates/` | `layout.html` and one template per page; `{{ name }}` is a value from `config.ts` |
| `static/` | Stylesheet, favicon and self-hosted fonts (SIL Open Font License) |
| `generator/` | The build, a local preview server and their tests |
| `contact/` | The Appwrite function the contact form posts to |
| `dist/` | Build output, ignored by git |

## Working on it

```bash
bun install --frozen-lockfile         # once, from the repository root
bun website/generator/serve.ts        # build, then preview at http://127.0.0.1:4321
bun website/generator/build.ts        # build into website/dist
(cd website && bun test generator contact)    # tests
(cd website && bunx @biomejs/biome@2.5.12 check . && bunx tsc -p tsconfig.json)
```

A template that uses a value `config.ts` does not have fails the build.

## Before launch

`unconfirmed` in `config.ts` lists the values that are still placeholders or proposals. The build prints a warning for each one, and in GitHub Actions the warnings show on the run. Remove a key from the list once it is final.

- **`plans`**: if a plan's price changes, change its Polar products too. The two must match. `memberLimit` must match the Community limit the server enforces.
- **`contactEmail`**: a mailbox that someone reads.
- **`domain`**: the custom domain configured for GitHub Pages. Deployed builds take canonical links from the base URL Pages reports (`SITE_BASE_URL`), so until the custom domain is live they point at `https://bitecodehq.github.io/zone/`. Local builds and the preview server fall back to `https://<domain>/`. Once it is final, set the success URL of both Polar checkout links to `https://<domain>/thanks/`.

## Contact form

The form on `/contact/` and `/pricing/` posts to `contactFormURL`, the `contact` function in the `bitecode` project on `appwrite.bitecode.ltd`, served at `contact.bitecode.ltd`. It checks the form, drops anything that fills the hidden `website` field, sends the message to `CONTACT_RECIPIENT` through Resend with the visitor as reply-to, and answers `303 See Other` to the `/contact/sent/` page it was posted from. It only redirects to origins listed in `CONTACT_ORIGINS`, so it cannot be used as an open redirect.

| Variable | Value |
| --- | --- |
| `RESEND_API_KEY` | A Resend key with sending access to `bitecode.ltd`, marked secret |
| `CONTACT_RECIPIENT` | `zone@bitecode.ltd` |
| `CONTACT_SENDER` | `Bitecode website <contact@bitecode.ltd>` |
| `CONTACT_ORIGINS` | `https://zone.bitecode.ltd,https://bitecodehq.github.io` |

The function runs on the `node-25` runtime with `main.ts` as its entrypoint. Node runs the TypeScript directly, so there is no build command. To deploy, upload the `contact/` directory without its tests as a new deployment.

## Deploying

`.github/workflows/website.yml` lints, typechecks and tests every pull request that touches `website/`, and on pushes to `main` builds the site and deploys it to GitHub Pages.

1. In the repository's **Settings → Pages**, set **Source** to **GitHub Actions**. Until then the deploy job fails.
2. Without a custom domain the site is served at `https://bitecodehq.github.io/zone/`. The build reads that base URL from Pages, so links and canonical links work there too.

### Adding a custom domain

1. At the DNS provider, add a `CNAME` record pointing the domain (for example `zone.bitecode.ltd`) at `bitecodehq.github.io`.
2. In **Settings → Pages**, enter the domain under **Custom domain**, wait for the DNS check, then tick **Enforce HTTPS**.
3. Set `domain` in `config.ts` to the same value.

Sites deployed from a GitHub Actions workflow take their custom domain from the repository settings and ignore a `CNAME` file, so the repository does not need one.
