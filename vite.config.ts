import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5180,
    // 必须 strict：tauri.conf.json 的 devUrl 固定指向 5180，若此处允许 vite 自动改端口，
    // 端口被占用时 vite 会静默换到 5181，而 Tauri 仍然去连 5180，表现为白屏且报错不指向真因。
    strictPort: true,
  },
});
