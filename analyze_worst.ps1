param([string]$Report = "audit_report.html", [int]$Limit = 15)
# Compatibility entry point; the maintained reader consumes current HTML reports.
python (Join-Path $PSScriptRoot 'scripts/analyze_report.py') $Report --limit $Limit
exit $LASTEXITCODE
