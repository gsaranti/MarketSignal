import { createApp } from "vue";
import App from "./App.vue";

// Fonts are bundled locally so the app never fetches them over the network;
// Latin subset only, weights matching colors_and_type.css.
import "@fontsource/source-serif-4/latin-400.css";
import "@fontsource/source-serif-4/latin-500.css";
import "@fontsource/source-serif-4/latin-600.css";
import "@fontsource/source-serif-4/latin-700.css";
import "@fontsource/public-sans/latin-400.css";
import "@fontsource/public-sans/latin-500.css";
import "@fontsource/public-sans/latin-600.css";
import "@fontsource/ibm-plex-mono/latin-400.css";
import "@fontsource/ibm-plex-mono/latin-500.css";

import "../market-signal-design-system/colors_and_type.css";

import { applyTheme, readDark } from "./theme";

// Before mount, so a dark-preference launch never flashes the light theme.
applyTheme(readDark());

createApp(App).mount("#app");
