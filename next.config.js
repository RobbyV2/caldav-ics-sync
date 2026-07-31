/** @type {import('next').NextConfig} */

const nextConfig = {
  output: 'standalone',
  trailingSlash: true,
  // The Rust server proxies to the dev server, so requests arrive with its origin.
  allowedDevOrigins: ['127.0.0.1', 'localhost'],
}
export default nextConfig
