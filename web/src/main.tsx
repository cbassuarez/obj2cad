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
  return (
    <MantineProvider theme={mantineTheme} forceColorScheme="light">
      <MotionConfig reducedMotion="user">
        <TooltipProvider>
          <Notifications position="bottom-center" limit={3} classNames={{ notification: "panel" }} />
          <App />
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
