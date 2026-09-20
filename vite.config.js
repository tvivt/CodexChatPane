import { defineConfig } from "vite";

export default defineConfig({
  server: {
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/.runtime/**"] },
  },
  build: {
    rollupOptions: { input: "index.html" },
  },
  clearScreen: false,
});
