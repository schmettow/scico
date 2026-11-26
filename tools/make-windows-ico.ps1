# Generates assets/logo/Scico.ico from Scico-square.png.
#
# Windows shortcuts, the Start menu, and the taskbar ask for an .ico, not a
# PNG. The ICO embeds the mark at the sizes Explorer and the shell pick from:
# 16, 24, 32, 48, 64, 128, and 256 px. Run from the repository root:
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools/make-windows-ico.ps1

param(
    [string]$Source = (Join-Path $PSScriptRoot '..\assets\logo\Scico-square.png'),
    [string]$Destination = (Join-Path $PSScriptRoot '..\assets\logo\Scico.ico')
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$sizes = 16, 24, 32, 48, 64, 128, 256
$sourceImage = [System.Drawing.Image]::FromFile((Resolve-Path $Source).Path)
$images = @()

try {
    foreach ($size in $sizes) {
        $bitmap = [System.Drawing.Bitmap]::new(
            $size,
            $size,
            [System.Drawing.Imaging.PixelFormat]::Format32bppArgb
        )
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
            $graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
            $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
            $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
            $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
            $graphics.DrawImage($sourceImage, 0, 0, $size, $size)
        } finally {
            $graphics.Dispose()
        }

        $stream = [System.IO.MemoryStream]::new()
        try {
            $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
            $images += [pscustomobject]@{ Size = $size; Bytes = $stream.ToArray() }
        } finally {
            $stream.Dispose()
            $bitmap.Dispose()
        }
    }
} finally {
    $sourceImage.Dispose()
}

# ICONDIR, then one ICONDIRENTRY per image, then the PNG payloads. A dimension
# of 256 is stored as 0, as the format reserves one byte per axis.
$destinationPath = [System.IO.Path]::GetFullPath($Destination)
$writer = [System.IO.BinaryWriter]::new([System.IO.File]::Create($destinationPath))
try {
    $writer.Write([UInt16]0)
    $writer.Write([UInt16]1)
    $writer.Write([UInt16]$images.Count)

    $offset = 6 + 16 * $images.Count
    foreach ($image in $images) {
        $dimension = if ($image.Size -ge 256) { 0 } else { $image.Size }
        $writer.Write([byte]$dimension)
        $writer.Write([byte]$dimension)
        $writer.Write([byte]0)
        $writer.Write([byte]0)
        $writer.Write([UInt16]1)
        $writer.Write([UInt16]32)
        $writer.Write([UInt32]$image.Bytes.Length)
        $writer.Write([UInt32]$offset)
        $offset += $image.Bytes.Length
    }

    foreach ($image in $images) {
        $writer.Write($image.Bytes)
    }
} finally {
    $writer.Dispose()
}

Write-Output "wrote $Destination ($($images.Count) sizes)"
