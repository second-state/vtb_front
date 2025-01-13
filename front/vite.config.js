import { defineConfig } from 'vite'

export default defineConfig({
    server: {
        proxy: {
            // Proxying websockets or socket.io
            '/ws': {
                target: 'ws://localhost:8000',
                ws: true
            }
        }
    }
})