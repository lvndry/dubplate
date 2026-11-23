import { defineConfig } from "astro/config";

export default defineConfig({
  site: "https://lvndry.github.io",
  base: "/dubplate/",
  trailingSlash: "ignore",
  compressHTML: false,
  build: { format: "file" },
});
