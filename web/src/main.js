import { jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/500.css";
import "@fontsource/ibm-plex-sans/600.css";
import "@fontsource/ibm-plex-mono/400.css";
import "@fontsource/ibm-plex-mono/500.css";
import "@fontsource-variable/instrument-sans";
import "./index.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { MantineProvider } from "@mantine/core";
import { Notifications } from "@mantine/notifications";
import { MotionConfig } from "motion/react";
import { TooltipProvider } from "@/components/ui/tooltip";
import { App } from "@/App";
import { mantineTheme } from "@/theme";
function Root() {
    return (_jsx(MantineProvider, { theme: mantineTheme, forceColorScheme: "light", children: _jsx(MotionConfig, { reducedMotion: "user", children: _jsxs(TooltipProvider, { children: [_jsx(Notifications, { position: "bottom-center", limit: 3, classNames: { notification: "panel" } }), _jsx(App, {})] }) }) }));
}
createRoot(document.getElementById("root")).render(_jsx(StrictMode, { children: _jsx(Root, {}) }));
