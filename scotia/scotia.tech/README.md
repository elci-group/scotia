# scotia.tech

Static website for the Scotia project.

## Files

- `index.html` — landing page
- `styles.css` — responsive theme-aware stylesheet
- `script.js` — theme toggle, copy buttons, scroll animations
- `docs/index.html` — documentation page

## Development

Open `index.html` directly in a browser, or serve locally:

```bash
python3 -m http.server 8080
```

Then visit `http://localhost:8080`.

## Theme

The site supports dark and light modes. The user's preference is persisted in `localStorage` and respects `prefers-color-scheme`.

## Deployment

This is a static site. Deploy the contents of this directory to any static host (GitHub Pages, Cloudflare Pages, Netlify, Vercel, S3, etc.).
