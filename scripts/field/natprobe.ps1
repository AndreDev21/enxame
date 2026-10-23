# Equivalente em PowerShell de natprobe.py (Windows sem Python). Só .NET.
#   .\natprobe.ps1 map
#   .\natprobe.ps1 punch -Peer IP:PORTA -At EPOCH [-Secs 12]
# Não imprima nem comite endereços IP públicos em relatórios.
param(
  [Parameter(Mandatory = $true, Position = 0)][ValidateSet('map', 'punch')][string]$Cmd,
  [string]$Peer, [double]$At = 0, [double]$Secs = 12, [int]$Port = 0
)
$ErrorActionPreference = 'Stop'
$stun = @(@('stun.l.google.com', 19302), @('stun1.l.google.com', 19302), @('stun.cloudflare.com', 3478), @('stun.nextcloud.com', 443))
$udp = New-Object System.Net.Sockets.UdpClient($Port)
$udp.Client.ReceiveTimeout = 2000

function Get-Mapped($server) {
  $tid = New-Object byte[] 12
  [System.Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($tid)
  $req = [byte[]](0x00, 0x01, 0x00, 0x00, 0x21, 0x12, 0xA4, 0x42) + $tid
  $ip = [System.Net.Dns]::GetHostAddresses($server[0]) | Where-Object { $_.AddressFamily -eq 'InterNetwork' } | Select-Object -First 1
  $ep = New-Object System.Net.IPEndPoint($ip, $server[1])
  [void]$udp.Send($req, $req.Length, $ep)
  $deadline = [DateTime]::UtcNow.AddSeconds(2)
  while ([DateTime]::UtcNow -lt $deadline) {
    try {
      $from = New-Object System.Net.IPEndPoint([System.Net.IPAddress]::Any, 0)
      $d = $udp.Receive([ref]$from)
    } catch { return @($ip.ToString(), $null) }
    if ($d.Length -lt 20) { continue }
    $same = $true
    for ($k = 0; $k -lt 12; $k++) { if ($d[8 + $k] -ne $tid[$k]) { $same = $false } }
    if (-not $same) { continue }
    $i = 20
    while ($i + 4 -le $d.Length) {
      $t = ($d[$i] * 256) + $d[$i + 1]; $l = ($d[$i + 2] * 256) + $d[$i + 3]
      if ($t -eq 0x0020 -and $l -ge 8) {
        $p = (($d[$i + 6] * 256) + $d[$i + 7]) -bxor 0x2112
        $a = @(($d[$i + 8] -bxor 0x21), ($d[$i + 9] -bxor 0x12), ($d[$i + 10] -bxor 0xA4), ($d[$i + 11] -bxor 0x42)) -join '.'
        return @($ip.ToString(), "${a}:$p")
      }
      $i += 4 + $l + ((4 - ($l % 4)) % 4)
    }
  }
  return @($ip.ToString(), $null)
}

if ($Cmd -eq 'map') {
  "local " + $udp.Client.LocalEndPoint.Port
  $seen = @()
  foreach ($s in $stun) {
    $r = Get-Mapped $s
    "{0} ({1}): {2}" -f $s[0], $r[0], $r[1]
    if ($r[1]) { $seen += $r[1] }
  }
  $ports = $seen | ForEach-Object { $_.Split(':')[1] } | Select-Object -Unique
  $ips = $seen | ForEach-Object { $_.Split(':')[0] } | Select-Object -Unique
  "RESUMO: {0} respostas, {1} IP(s) público(s), {2} porta(s) distinta(s)" -f $seen.Count, @($ips).Count, @($ports).Count
  if ($seen.Count -eq 0) { 'mapeamento: sem resposta' }
  elseif (@($ports).Count -eq 1) { 'mapeamento: independente do destino (cone)' }
  else { 'mapeamento: DEPENDENTE do destino (simétrico)' }
} else {
  $mine = (Get-Mapped $stun[0])[1]
  "MEU $mine"
  $ph = $Peer.Split(':')
  $peerEp = New-Object System.Net.IPEndPoint([System.Net.IPAddress]::Parse($ph[0]), [int]$ph[1])
  $nowEpoch = [double]([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()) / 1000
  "saindo em {0:N1}s" -f ($At - $nowEpoch)
  while (([double]([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()) / 1000) -lt $At) { Start-Sleep -Milliseconds 10 }
  $udp.Client.ReceiveTimeout = 50
  $t0 = [DateTime]::UtcNow; $got = 0; $sent = 0; $first = $null; $diff = $false; $next = [DateTime]::UtcNow
  while (([DateTime]::UtcNow - $t0).TotalSeconds -lt $Secs) {
    if ([DateTime]::UtcNow -ge $next) {
      $b = [Text.Encoding]::ASCII.GetBytes("enxame-probe $sent")
      [void]$udp.Send($b, $b.Length, $peerEp); $sent++; $next = [DateTime]::UtcNow.AddMilliseconds(200)
    }
    try {
      $from = New-Object System.Net.IPEndPoint([System.Net.IPAddress]::Any, 0)
      $d = $udp.Receive([ref]$from)
      if ([Text.Encoding]::ASCII.GetString($d).StartsWith('enxame-probe')) {
        $got++
        if ($from.Port -ne $peerEp.Port) { $diff = $true }
        if ($null -eq $first) { $first = [math]::Round(([DateTime]::UtcNow - $t0).TotalSeconds, 2) }
      }
    } catch {}
  }
  "RESULTADO: enviou $sent, recebeu $got, primeiro em ${first}s, porta diferente: $diff"
}
