# Tallybook website

Live at https://tallybook-app.netlify.app. Built from the 2025 Hugo starter (Hugo 0.147, Tailwind + SCSS).

```sh
npm install
npm run dev     # SCSS + Tailwind watchers and hugo server on http://localhost:1313
npm run build   # compiles CSS, then hugo --gc --minify into public/
```

- **Styles live in `assets/scss/`.** `main.scss` holds the shared styles; `pages/*.scss`
  are per page and pulled in by a page's `css:` front matter. `assets/css` and `static/css`
  are build output.
- **Prices, checkout links and the download version** are in `hugo.toml` under `[params]`.
  Bump `version` on every release so the download buttons point at the new files.
- **Questions** shown on the home and pricing pages are in `data/faq.yaml`.
- **Screenshots** in `static/images/screens/` come from the Northwind Studio demo book and
  a seeded household book, captured at 1440×900 @2x.
- **Deploying on Netlify:** set the site's base directory to `website`; `netlify.toml` does the rest.
