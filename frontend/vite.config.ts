import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";
import path from "path";

export default defineConfig({
  plugins: [tailwindcss()],
  resolve: {
    alias: {
      "~bridge": path.resolve(__dirname, "./bridge.gen"),
    },
  },
});
