// The developer documentation, built by Astro with Starlight and published
// to GitHub Pages at https://agentplot.github.io/sluice by
// .github/workflows/pages.yml. The decisions section is generated from
// docs/adr by scripts/adrs.mjs before every build.

import starlight from "@astrojs/starlight";
import { defineConfig } from "astro/config";
import rehypeMermaid from "rehype-mermaid";
import starlightLinksValidator from "starlight-links-validator";

// ```mermaid blocks are drawn as SVG at build time, a light and a dark one
// in a <picture>, so a page needs no script to show them. Drawing needs a
// Chromium: Playwright's (PLAYWRIGHT_BROWSERS_PATH in the devenv shell).
// The dark one's arrows and labels are near white on the dark page.
const dark = {
  theme: "dark",
  themeVariables: {
    lineColor: "#d8dce4",
    textColor: "#f2f3f5",
    signalColor: "#d8dce4",
    signalTextColor: "#f2f3f5",
    edgeLabelBackground: "#2a2f38",
    titleColor: "#f2f3f5",
  },
};

export default defineConfig({
  site: "https://agentplot.github.io",
  base: "/sluice",
  // A page that moved, from where links to it still point.
  redirects: {
    "/guides/github-actions": "/sluice/guides/pools/",
  },
  markdown: {
    rehypePlugins: [
      [rehypeMermaid, { strategy: "img-svg", dark, launchOptions: { channel: "chromium" } }],
    ],
  },
  integrations: [
    starlight({
      title: "sluice",
      description: "A merge queue for agents working in parallel worktrees.",
      logo: {
        light: "./src/assets/mark-light.svg",
        dark: "./src/assets/mark-dark.svg",
        alt: "sluice",
      },
      favicon: "/favicon.svg",
      // The family's header (its bar above Starlight's), footer and status
      // banner, from @agentplot/site-theme; the start page's hero, with the
      // animated diagram, is sluice's own.
      components: {
        Header: "@agentplot/site-theme/starlight/Header.astro",
        Footer: "@agentplot/site-theme/starlight/Footer.astro",
        Banner: "@agentplot/site-theme/starlight/Banner.astro",
        Hero: "./src/components/Hero.astro",
      },
      // The family's type, ink and accent, then the brand of docs/brand,
      // whose exact greens are sluice's own.
      customCss: ["@agentplot/site-theme/starlight.css", "./src/styles/brand.css"],
      // The social card, a copy of docs/brand/social-preview.png.
      head: [
        {
          tag: "meta",
          attrs: { property: "og:image", content: "https://agentplot.github.io/sluice/og.png" },
        },
        { tag: "meta", attrs: { name: "twitter:card", content: "summary_large_image" } },
      ],
      social: [{ icon: "github", label: "GitHub", href: "https://github.com/agentplot/sluice" }],
      editLink: {
        baseUrl: "https://github.com/agentplot/sluice/edit/main/site/",
      },
      plugins: [starlightLinksValidator()],
      sidebar: [
        { label: "Start", link: "/" },
        { label: "Guides", items: [{ autogenerate: { directory: "guides" } }] },
        { label: "Reference", items: [{ autogenerate: { directory: "reference" } }] },
        { label: "Architecture", items: [{ autogenerate: { directory: "architecture" } }] },
        {
          label: "Decisions",
          collapsed: true,
          items: [{ autogenerate: { directory: "decisions" } }],
        },
        { label: "Roadmap", link: "/roadmap/" },
      ],
    }),
  ],
});
