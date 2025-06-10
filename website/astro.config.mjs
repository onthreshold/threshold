// @ts-check
import { defineConfig, fontProviders } from "astro/config";
import tailwindcss from "@tailwindcss/vite";
import mdx from "@astrojs/mdx";
import keystatic from "@keystatic/astro";

import react from "@astrojs/react";

import icon from "astro-icon";

// https://astro.build/config
export default defineConfig({
  vite: {
    plugins: [tailwindcss()],
    resolve: {
      alias: {
        "@": "/src",
      },
    },
    optimizeDeps: {},
  },
  experimental: {
    fonts: [
      {
        provider: fontProviders.google(),
        name: "Lora",
        cssVariable: "--font-lora",
      },
    ],
  },
  integrations: [mdx(), keystatic(), react(), icon()],
});
