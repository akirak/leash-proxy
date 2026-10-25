import { defineConfig } from 'vitepress'

// Set `base` to '/<repo-name>/' when deploying to a GitHub Pages project site.
export default defineConfig({
  lang: 'en-US',
  title: 'leash-proxy',
  description:
    'A Rust forward proxy for micro VM agents, controlled by Cedar policies with structured audit logging.',
  themeConfig: {
    siteTitle: 'leash-proxy',
    nav: [
      { text: 'Guide', link: '/guide/introduction' },
      { text: 'Roadmap', link: '/roadmap' }
    ],
    sidebar: {
      '/guide/': [
        {
          text: 'Guide',
          items: [
            { text: 'Introduction', link: '/guide/introduction' },
            { text: 'Architecture', link: '/guide/architecture' },
            { text: 'Getting started', link: '/guide/getting-started' },
            { text: 'Configuration', link: '/guide/configuration' },
            { text: 'Cedar policies', link: '/guide/policies' },
            { text: 'Audit log', link: '/guide/audit-log' }
          ]
        }
      ]
    },
    socialLinks: [
      { icon: 'github', link: 'https://github.com/akirakomamura/leash-proxy' }
    ],
    outline: { level: [2, 3] }
  }
})
