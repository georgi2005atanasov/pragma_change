import type { Metadata } from "next";
import "@fontsource-variable/dm-sans/wght.css";
import "@fontsource-variable/space-grotesk/wght.css";
import "@fontsource/ibm-plex-mono/400.css";
import "./globals.css";
export const metadata: Metadata = {
  title: "PragmaChange · Behavioral Security",
  description:
    "Configure, train, and deploy anomaly detection at the proxy layer.",
};
export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <script
          dangerouslySetInnerHTML={{
            __html:
              "try{if(localStorage.getItem('pragma.sidebar.collapsed')==='true')document.documentElement.dataset.sidebarCollapsed='true'}catch(e){}",
          }}
        />
      </head>
      <body>{children}</body>
    </html>
  );
}
