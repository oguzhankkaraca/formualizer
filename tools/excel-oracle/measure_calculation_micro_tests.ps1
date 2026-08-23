param(
    [Parameter(Mandatory = $true)]
    [string]$ManifestPath,
    [string]$OutputPath = "",
    [int[]]$MaxIterations = @(1, 2, 3, 5, 10, 20, 50, 100),
    [int]$Runs = 7,
    [double]$MaxChange = 0.001,
    [string]$CaseId = ""
)

$ErrorActionPreference = "Stop"

function Release-ComObject {
    param([object]$Object)
    if ($null -ne $Object -and [System.Runtime.InteropServices.Marshal]::IsComObject($Object)) {
        [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($Object)
    }
}

function Wait-Calculation {
    param([object]$Excel)
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    while ([int]$Excel.CalculationState -eq 1) {
        if ($watch.Elapsed.TotalSeconds -gt 120) {
            throw "Excel calculation did not reach a non-calculating state; state=$([int]$Excel.CalculationState)"
        }
        Start-Sleep -Milliseconds 5
    }
}

function Measure-Calculation {
    param([object]$Excel)
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $Excel.Calculate()
    Wait-Calculation $Excel
    $watch.Stop()
    return [math]::Round($watch.Elapsed.TotalMilliseconds, 3)
}

function Measure-FullRebuild {
    param([object]$Excel)
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $Excel.CalculateFullRebuild()
    Wait-Calculation $Excel
    $watch.Stop()
    return [math]::Round($watch.Elapsed.TotalMilliseconds, 3)
}

function Set-CellValue {
    param([object]$Workbook, [string]$Sheet, [string]$Address, [object]$Value)
    $worksheet = $Workbook.Worksheets.Item($Sheet)
    $cell = $worksheet.Range($Address)
    try {
        if ($Value -is [string]) { $cell.Value2 = [string]$Value }
        elseif ($null -eq $Value) { [void]$cell.ClearContents() }
        else { $cell.Value2 = [double]$Value }
    }
    finally {
        Release-ComObject $cell
        Release-ComObject $worksheet
    }
}

function Get-TargetSnapshot {
    param([object]$Workbook, [string[]]$Targets)
    $result = [ordered]@{}
    foreach ($target in $Targets) {
        $separator = $target.LastIndexOf("!")
        $sheet = $target.Substring(0, $separator)
        $address = $target.Substring($separator + 1)
        $worksheet = $Workbook.Worksheets.Item($sheet)
        $cell = $worksheet.Range($address)
        try {
            $result[$target] = [ordered]@{
                value2 = $cell.Value2
                text   = [string]$cell.Text
            }
        }
        finally {
            Release-ComObject $cell
            Release-ComObject $worksheet
        }
    }
    return $result
}

function New-ConfiguredExcel {
    param([int]$Iterations, [double]$Change)
    $excel = New-Object -ComObject Excel.Application
    $excel.Visible = $false
    $excel.DisplayAlerts = $false
    try { $excel.AutomationSecurity = 3 } catch {}
    $bootstrap = $excel.Workbooks.Add()
    try {
        $excel.Calculation = -4135
        $excel.Iteration = $true
        $excel.MaxIterations = $Iterations
        $excel.MaxChange = $Change
    }
    finally {
        try { $bootstrap.Close($false) } catch {}
        Release-ComObject $bootstrap
    }
    return $excel
}

function Save-Checkpoint {
    param([object[]]$Rows)
    $report = [ordered]@{
        schema           = "formualizer.excel-calculation-micro-results/v1"
        generated_at_utc = [DateTime]::UtcNow.ToString("o")
        manifest         = $resolvedManifest
        max_change       = $MaxChange
        runs             = $Runs
        max_iterations   = $MaxIterations
        results          = $Rows
    }
    $json = $report | ConvertTo-Json -Depth 30
    [System.IO.File]::WriteAllText($OutputPath, $json + [Environment]::NewLine, [System.Text.UTF8Encoding]::new($false))
}

$resolvedManifest = (Resolve-Path $ManifestPath).Path
$manifest = Get-Content -Raw $resolvedManifest | ConvertFrom-Json
$manifestDirectory = Split-Path -Parent $resolvedManifest
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $OutputPath = Join-Path $manifestDirectory "micro-results.json"
}

$results = @()
foreach ($case in $manifest.cases) {
    if (-not [string]::IsNullOrWhiteSpace($CaseId) -and [string]$case.id -ne $CaseId) {
        continue
    }
    $casePath = Join-Path $manifestDirectory ([string]$case.id + ".xlsx")
    if (-not (Test-Path $casePath)) {
        throw "Micro workbook not found: $casePath"
    }
    foreach ($iterationLimit in $MaxIterations) {
        Write-Output "Starting $($case.id) maxIterations=$iterationLimit"
        $excel = New-ConfiguredExcel -Iterations $iterationLimit -Change $MaxChange
        $workbook = $null
        try {
            $openWatch = [System.Diagnostics.Stopwatch]::StartNew()
            $workbook = $excel.Workbooks.Open($casePath)
            $openWatch.Stop()
            $openMs = [math]::Round($openWatch.Elapsed.TotalMilliseconds, 3)
            Write-Output "    opened in $openMs ms"
            $excel.Calculation = -4135
            $excel.Iteration = $true
            $excel.MaxIterations = $iterationLimit
            $excel.MaxChange = $MaxChange
            for ($run = 1; $run -le $Runs; $run++) {
                Write-Output "  Run $run/$Runs"
                $setupRebuildMs = Measure-FullRebuild $excel
                $initialMs = Measure-Calculation $excel
                $initialTargets = Get-TargetSnapshot -Workbook $workbook -Targets $case.targets
                $mutationMs = $null
                $mutationTargets = $null
                $noopMs = $null
                $noopTargets = $null
                if ($case.mutations.Count -gt 0) {
                    $mutation = $case.mutations[0]
                    Set-CellValue -Workbook $workbook -Sheet ([string]$mutation.sheet) -Address ([string]$mutation.address) -Value $mutation.value
                    $mutationMs = Measure-Calculation $excel
                    $mutationTargets = Get-TargetSnapshot -Workbook $workbook -Targets $case.targets
                    $noopMs = Measure-Calculation $excel
                    $noopTargets = Get-TargetSnapshot -Workbook $workbook -Targets $case.targets
                    if ($run -lt $Runs) {
                        $baseline = $case.cells.([string]$mutation.sheet).([string]$mutation.address)
                        Set-CellValue -Workbook $workbook -Sheet ([string]$mutation.sheet) -Address ([string]$mutation.address) -Value $baseline
                    }
                }
                else {
                    $noopMs = Measure-Calculation $excel
                    $noopTargets = Get-TargetSnapshot -Workbook $workbook -Targets $case.targets
                }
                $results += [ordered]@{
                    case_id               = [string]$case.id
                    iteration_limit       = $iterationLimit
                    run                   = $run
                    open_ms               = if ($run -eq 1) { $openMs } else { 0 }
                    setup_full_rebuild_ms = $setupRebuildMs
                    initial_calculate_ms  = $initialMs
                    mutation_calculate_ms = $mutationMs
                    noop_calculate_ms     = $noopMs
                    initial_targets       = $initialTargets
                    mutation_targets      = $mutationTargets
                    noop_targets          = $noopTargets
                }
                Save-Checkpoint -Rows $results
            }
        }
        finally {
            if ($null -ne $workbook) { try { $workbook.Close($false) } catch {} }
            try { $excel.Quit() } catch {}
            Release-ComObject $workbook
            Release-ComObject $excel
            [GC]::Collect()
            [GC]::WaitForPendingFinalizers()
        }
    }
}

Save-Checkpoint -Rows $results
Write-Output "Generated $OutputPath"
