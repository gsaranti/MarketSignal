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

createApp(App).mount("#app");
