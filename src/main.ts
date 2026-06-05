import { createApp } from "vue";
import App from "./App.vue";

// Self-hosted typefaces — bundled locally so the app never fetches fonts over
// the network (local-first). Replaces the design system's former remote
// Google Fonts @import. Latin subset only (the app's content language); weights
// match the design tokens in colors_and_type.css.
import "@fontsource/source-serif-4/latin-400.css";
import "@fontsource/source-serif-4/latin-500.css";
import "@fontsource/source-serif-4/latin-600.css";
import "@fontsource/source-serif-4/latin-700.css";
import "@fontsource/public-sans/latin-400.css";
import "@fontsource/public-sans/latin-500.css";
import "@fontsource/public-sans/latin-600.css";
import "@fontsource/ibm-plex-mono/latin-400.css";
import "@fontsource/ibm-plex-mono/latin-500.css";

import "../market-signal-design-system/project/colors_and_type.css";

import { applyTheme, readDark } from "./theme";

// Apply the saved appearance before mount so a dark-preference launch never
// flashes the light theme — localStorage is synchronous, so the webview's first
// paint already has `data-theme` set.
applyTheme(readDark());

createApp(App).mount("#app");
