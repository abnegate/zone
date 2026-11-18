# Zone website

The public site: home, pricing, licensing, the post-checkout thank-you page and a 404 page. It is plain HTML and CSS, rendered by a small Bun script that fills shared values into the templates. There is no client-side JavaScript, no cookies and no tracking.

| Path | What |
| --- | --- |
| `config.ts` | Every value that may change: price, Polar checkout link, contact address, domain, repository |
| `templates/` | `layout.html` and one template per page; `{{ name }}` is a value from `config.ts` |
| `static/` | Stylesheet, favicon and self-hosted fonts (SIL Open Font License) |
| `generator/` | The build, a local preview server and their tests |
| `dist/` | Build output, ignored by git |

## Working on it

```bash
bun website/generator/serve.ts        # build, then preview at http://127.0.0.1:4321
bun website/generator/build.ts        # build into website/dist
(cd website && bun test generator)    # tests
(cd website && biome check . && bunx tsc -p tsconfig.json)
```

A template that uses a value `config.ts` does not have fails the build.

## Before launch

`unconfirmed` in `config.ts` lists the values that are still placeholders or proposals. The build prints a warning for each one, and in GitHub Actions the warnings show on the run. Remove a key from the list once it is final.

- **`checkoutURL`**: create the Zone Enterprise product in Polar, create a checkout link for it, and paste the link here. Set the checkout link's success URL to `https://<domain>/thanks/`.
- **`price`**: the price on the site must match the Polar product.
- **`contactEmail`**: a mailbox that someone reads.
- **`domain`**: used for canonical links. It must match the custom domain configured for GitHub Pages.

## Deploying

`.github/workflows/website.yml` lints, typechecks and tests every pull request that touches `website/`, and on pushes to `main` builds the site and deploys it to GitHub Pages.

1. In the repository's **Settings → Pages**, set **Source** to **GitHub Actions**. Until then the deploy job fails.
2. Without a custom domain the site is served at `https://abnegate.github.io/zone/`. The build reads that base path from Pages, so links work there too.

### Adding a custom domain

1. At the DNS provider, add a `CNAME` record pointing the domain (for example `zone.abnegate.dev`) at `abnegate.github.io`.
2. In **Settings → Pages**, enter the domain under **Custom domain**, wait for the DNS check, then tick **Enforce HTTPS**.
3. Set `domain` in `config.ts` to the same value.

Sites deployed from a GitHub Actions workflow take their custom domain from the repository settings and ignore a `CNAME` file, so the repository does not need one.
