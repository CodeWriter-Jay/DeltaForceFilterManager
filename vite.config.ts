import { defineConfig } from "vite";
export default defineConfig({
  root: "web",
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { outDir: "../dist", emptyOutDir: true },
});
