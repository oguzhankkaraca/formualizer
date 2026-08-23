param(
    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory
)

$ErrorActionPreference = "Stop"
$generator = Join-Path $PSScriptRoot "create_calculation_micro_tests.py"
& python $generator --output-directory $OutputDirectory
if ($LASTEXITCODE -ne 0) {
    throw "Micro-workbook generator failed with exit code $LASTEXITCODE"
}
