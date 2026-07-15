import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// `npm run dev` proxies API calls to the learn-7 server; `npm run build`
// emits dist/, which the server itself serves.
export default defineConfig({
  plugins: [react()],
  server: { proxy: { '/api': 'http://localhost:3007' } },
})
