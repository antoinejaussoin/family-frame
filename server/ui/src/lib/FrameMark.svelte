<script>
  let { size = 56 } = $props()
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 64 64"
  aria-hidden="true"
  class="shrink-0 drop-shadow-sm"
>
  <rect x="4" y="6" width="56" height="52" rx="8" fill="#d4654a" />
  <rect x="12" y="14" width="40" height="32" fill="#fff6e4" />
  <circle cx="42" cy="24" r="5.5" fill="#efc15a" />
  <path d="M12 46V34.5l8.5-8.5 8 7 7.5-9.5L52 34v12z" fill="#4f8f68" />
</svg>
