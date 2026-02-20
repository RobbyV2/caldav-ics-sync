import Script from 'next/script'
import './globals.css'

export const metadata = {
  title: 'CalDAV/ICS Sync',
  description: 'Bidirectional CalDAV and ICS synchronization: CalDAV-to-ICS and ICS-to-CalDAV',
}

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" data-theme="dark" suppressHydrationWarning>
      <head>
        <link rel="stylesheet" href="/windows-ui/app-config.css" />
        <link rel="stylesheet" href="/windows-ui/windows-ui.min.css" />
        <link rel="stylesheet" href="/windows-ui/winui-icons.min.css" />
      </head>
      <body>
        {children}
        <Script src="/windows-ui/windows-ui.min.js" strategy="afterInteractive" />
      </body>
    </html>
  )
}
