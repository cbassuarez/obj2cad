import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/500.css";
import "@fontsource/ibm-plex-sans/600.css";
import "@fontsource/ibm-plex-mono/400.css";
import "@fontsource/ibm-plex-mono/500.css";
import "@fontsource-variable/instrument-sans";
import "./index.css";

import { StrictMode, useState } from "react";
import { createRoot } from "react-dom/client";
import { MantineProvider } from "@mantine/core";
import { Notifications } from "@mantine/notifications";
import { MotionConfig } from "motion/react";
import { TooltipProvider } from "@/components/ui/tooltip";
import { App } from "@/App";
import { mantineTheme } from "@/theme";
import { applyTheme, loadTheme, type Theme } from "@/lib/settings";

function Root() {
  const [theme, setTheme] = useState<Theme>(loadTheme);
  const toggle = () => {
    const next = theme === "dark" ? "light" : "dark";
    applyTheme(next);
    setTheme(next);
  };
  return (
    <MantineProvider theme={mantineTheme} forceColorScheme={theme}>
      <MotionConfig reducedMotion="user">
        <TooltipProvider>
          <Notifications position="bottom-center" limit={3} classNames={{ notification: "panel !rounded-[14px]" }} />
          <App theme={theme} onToggleTheme={toggle} />
        </TooltipProvider>
      </MotionConfig>
    </MantineProvider>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Root />
  </StrictMode>,
);
