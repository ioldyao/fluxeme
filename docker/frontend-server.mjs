import http from 'node:http'
import https from 'node:https'
import net from 'node:net'
import fs from 'node:fs/promises'
import path from 'node:path'
import { URL } from 'node:url'

const port = Number(process.env.PORT || '8081')
const staticRoot = path.resolve(process.env.STATIC_ROOT || '/app/web/portal')
const routeBase = normalizeRouteBase(process.env.ROUTE_BASE || '/')
const apiOrigin = new URL(process.env.API_ORIGIN || 'http://127.0.0.1:8080')
const proxyPrefixes = ['/api', '/v1', '/health', '/healthz', '/readyz', '/tokenize', '/detokenize', '/responses']

function normalizeRouteBase(base) {
  if (!base || base === '/') return '/'
  const normalized = base.startsWith('/') ? base : `/${base}`
  return normalized.endsWith('/') ? normalized.slice(0, -1) : normalized
}

function shouldProxy(pathname) {
  return proxyPrefixes.some((prefix) => pathname === prefix || pathname.startsWith(`${prefix}/`))
}

function stripRoutePrefix(pathname) {
  if (routeBase === '/') {
    return pathname
  }

  if (pathname === routeBase || pathname === `${routeBase}/`) {
    return '/'
  }

  if (pathname.startsWith(`${routeBase}/`)) {
    return pathname.slice(routeBase.length)
  }

  return null
}

function mimeFor(filePath) {
  if (filePath.endsWith('.html')) return 'text/html; charset=utf-8'
  if (filePath.endsWith('.js')) return 'application/javascript; charset=utf-8'
  if (filePath.endsWith('.css')) return 'text/css; charset=utf-8'
  if (filePath.endsWith('.svg')) return 'image/svg+xml'
  if (filePath.endsWith('.json')) return 'application/json; charset=utf-8'
  if (filePath.endsWith('.png')) return 'image/png'
  if (filePath.endsWith('.jpg') || filePath.endsWith('.jpeg')) return 'image/jpeg'
  if (filePath.endsWith('.woff2')) return 'font/woff2'
  if (filePath.endsWith('.woff')) return 'font/woff'
  if (filePath.endsWith('.ttf')) return 'font/ttf'
  return 'application/octet-stream'
}

async function tryServeRelative(relativePath, res) {
  const normalized = path.posix.normalize(relativePath)
  const resolvedPath = path.resolve(staticRoot, `.${normalized}`)

  if (!resolvedPath.startsWith(staticRoot)) {
    return false
  }

  try {
    const stat = await fs.stat(resolvedPath)
    if (!stat.isFile()) {
      return false
    }
    const bytes = await fs.readFile(resolvedPath)
    res.writeHead(200, { 'Content-Type': mimeFor(resolvedPath) })
    res.end(bytes)
    return true
  } catch {
    return false
  }
}

async function serveIndex(res) {
  const indexPath = path.join(staticRoot, 'index.html')
  try {
    const bytes = await fs.readFile(indexPath)
    res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' })
    res.end(bytes)
  } catch {
    res.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' })
    res.end('Not Found')
  }
}

function proxyHttp(req, res) {
  const client = apiOrigin.protocol === 'https:' ? https : http
  const headers = { ...req.headers, host: apiOrigin.host }
  const upstream = client.request(
    {
      protocol: apiOrigin.protocol,
      hostname: apiOrigin.hostname,
      port: apiOrigin.port || (apiOrigin.protocol === 'https:' ? 443 : 80),
      method: req.method,
      path: req.url,
      headers,
    },
    (upstreamRes) => {
      res.writeHead(upstreamRes.statusCode || 502, upstreamRes.headers)
      upstreamRes.pipe(res)
    },
  )

  upstream.on('error', (error) => {
    res.writeHead(502, { 'Content-Type': 'text/plain; charset=utf-8' })
    res.end(`Upstream proxy error: ${error.message}`)
  })

  req.pipe(upstream)
}

const server = http.createServer(async (req, res) => {
  const origin = req.headers.host ? `http://${req.headers.host}` : `http://127.0.0.1:${port}`
  const requestUrl = new URL(req.url || '/', origin)
  const { pathname } = requestUrl

  if (shouldProxy(pathname)) {
    proxyHttp(req, res)
    return
  }

  if (routeBase !== '/' && pathname === '/') {
    res.writeHead(302, { Location: `${routeBase}/` })
    res.end()
    return
  }

  const relativePath = stripRoutePrefix(pathname)
  if (relativePath === null) {
    res.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' })
    res.end('Not Found')
    return
  }

  const served = await tryServeRelative(relativePath, res)
  if (served) {
    return
  }

  await serveIndex(res)
})

server.on('upgrade', (req, socket, head) => {
  const origin = req.headers.host ? `http://${req.headers.host}` : `http://127.0.0.1:${port}`
  const requestUrl = new URL(req.url || '/', origin)
  if (!shouldProxy(requestUrl.pathname)) {
    socket.destroy()
    return
  }

  const upstream = net.connect(
    Number(apiOrigin.port || (apiOrigin.protocol === 'https:' ? 443 : 80)),
    apiOrigin.hostname,
    () => {
      let raw = `${req.method} ${req.url} HTTP/${req.httpVersion}\r\n`
      raw += `host: ${apiOrigin.host}\r\n`
      for (const [key, value] of Object.entries(req.headers)) {
        if (key.toLowerCase() === 'host') continue
        if (Array.isArray(value)) {
          for (const item of value) {
            raw += `${key}: ${item}\r\n`
          }
        } else if (value !== undefined) {
          raw += `${key}: ${value}\r\n`
        }
      }
      raw += '\r\n'
      upstream.write(raw)
      if (head.length > 0) {
        upstream.write(head)
      }
      socket.pipe(upstream).pipe(socket)
    },
  )

  upstream.on('error', () => {
    socket.destroy()
  })
})

server.listen(port, '0.0.0.0', () => {
  console.log(`frontend server listening on ${port}, base=${routeBase}, root=${staticRoot}`)
})
