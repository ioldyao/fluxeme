export type UnauthorizedHandler = {
  clearSession: () => void
  loginPath: string
  onUnauthorized?: () => void
}

export function handleUnauthorized({ clearSession, loginPath, onUnauthorized }: UnauthorizedHandler) {
  clearSession()
  onUnauthorized?.()
  window.location.href = loginPath
}
