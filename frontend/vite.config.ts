import { defineConfig } from 'vite'
import { devtools } from '@tanstack/devtools-vite'
import { tanstackRouter } from '@tanstack/router-plugin/vite'
import svgr from "vite-plugin-svgr";
import viteReact from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

const config = defineConfig({
  resolve: { tsconfigPaths: true },
  optimizeDeps: {
    include: [
      'use-sync-external-store/shim',
      'use-sync-external-store/shim/with-selector',
    ],
    // @sunstead/ui is source-only and its components import sibling modules by
    // relative path. Pre-bundling a deep entry (e.g. `use-theme`) creates a
    // second copy of shared React contexts, so `useTheme` reads a different
    // context than the one `ThemeProvider` writes. Keep the package as source.
    exclude: ['@sunstead/ui'],
  },
  plugins: [
    devtools(),
    tailwindcss(),
    tanstackRouter({ target: 'react', autoCodeSplitting: true }),
    viteReact(),
    svgr(),
  ],
})

export default config
