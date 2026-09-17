import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig, loadEnv } from 'vite'

/** Paths owned by the Rust server. Leave `/` to Vite so HMR keeps working. */
const BACKEND_PATHS = [
  '/api',
  '/static',
  '/dashboard',
  '/health',
  '/debug/frames',
]

function familyFrameVersion() {
  const fromEnv = (process.env.FAMILY_FRAME_VERSION || '').trim()
  if (fromEnv) return fromEnv
  const here = dirname(fileURLToPath(import.meta.url))
  try {
    return readFileSync(resolve(here, '../../VERSION'), 'utf8').trim()
  } catch {
    return '0.0.0'
  }
}

function backendProxy(target) {
  const toBackend = {
    target,
    changeOrigin: true,
    // First dashboard raster waits on Chrome; photo uploads can be large.
    timeout: 300_000,
    proxyTimeout: 300_000,
  }
  return Object.fromEntries(BACKEND_PATHS.map((path) => [path, { ...toBackend }]))
}

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), '')
  const target = env.EINK_API || process.env.EINK_API || 'http://127.0.0.1:8765'
  const proxy = backendProxy(target)

  return {
    plugins: [tailwindcss(), svelte()],
    define: {
      'import.meta.env.APP_VERSION': JSON.stringify(familyFrameVersion()),
    },
    clearScreen: false,
    server: {
      port: 5173,
      strictPort: true,
      host: true,
      proxy,
    },
    preview: {
      port: 4173,
      strictPort: true,
      host: true,
      proxy,
    },
    build: {
      outDir: 'dist',
      emptyOutDir: true,
    },
  }
})
