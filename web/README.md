# Project landing page

Single-file static site introducing Verified Anchor to a Solana-dev audience.

- `index.html` — the page itself. No build step, no framework, no dependencies.
- Open directly in a browser, or serve with any static HTTP server:

```bash
# Python (already installed on most systems)
cd web && python3 -m http.server 8000
# then open http://localhost:8000
```

## Hosting on GitHub Pages

1. In the repository settings on GitHub, go to **Settings → Pages**.
2. Under **Build and deployment**, set source to **Deploy from a branch**.
3. Branch: `master`, folder: `/web`.
4. Save.

The site will be live at <https://parthrathix0.github.io/Verified-Anchor/> within a minute.

(If you'd rather serve from `gh-pages` branch or root, the page is self-contained — copy `index.html` anywhere.)

## Editing

The page is one file. All CSS lives in a `<style>` block at the top; markup is in `<body>` after it. There is no JavaScript and no external scripts; only Google Fonts (Inter + JetBrains Mono) are fetched at load time.

Sections (in order, IDs map to the nav):

- Hero
- `#what` — what Verified Anchor is, in plain English
- `#why` — the problem, with four real CVE cards
- `#how` — what the user writes (side-by-side code)
- `#chain` — the verification chain (5-step diagram)
- `#audit` — the axiom-check command
- `#boundary` — what's proven vs not
- Footer
