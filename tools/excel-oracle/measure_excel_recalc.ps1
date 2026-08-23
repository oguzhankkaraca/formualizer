param(
    [Parameter(Mandatory = $true)]
    [string]$WorkbookPath,
    [string]$OutputPath = "",
    [string]$InputSheet = "Inputs",
    [string]$InputAddress = "F7",
    [double]$InputValue = 300,
    [string]$UnrelatedSheet = "Inputs",
    [string]$UnrelatedAddress = "O58",
    [string[]]$ObservationTargets = @(
        "CashFlow Inputs!J23",
        "CashFlow Inputs!J25",
        "CashFlow Inputs!N105",
        "CashFlow Inputs!N106",
        "CashFlow Engine!K29",
        "CashFlow Engine!K40",
        "Outputs!D42"
    ),
    [int[]]$MaxIterations = @(1, 2, 3, 5, 10, 20, 50, 100),
    [double]$MaxChange = 0.001,
    [int]$Runs = 7,
    [bool]$MultiThreaded = $true
)

$ErrorActionPreference = "Stop"

function Release-ComObject {
    param([object]$Object)
    if ($null -ne $Object -and [System.Runtime.InteropServices.Marshal]::IsComObject($Object)) {
        [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($Object)
    }
}

function Measure-Action {
    param([scriptblock]$Action)
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    & $Action
    $watch.Stop()
    return [ordered]@{
        elapsed_ms = [math]::Round($watch.Elapsed.TotalMilliseconds, 3)
    }
}

function Wait-Calculation {
    param([object]$Excel)
    while ([int]$Excel.CalculationState -eq 1) {
        Start-Sleep -Milliseconds 10
    }
}

function Get-ExcelSettings {
    param([object]$Excel)
    $multiThreaded = $null
    try {
        $mt = $Excel.MultiThreadedCalculation
        $multiThreaded = [ordered]@{
            enabled      = [bool]$mt.Enabled
            thread_mode  = [int]$mt.ThreadMode
            thread_count = [int]$mt.ThreadCount
        }
        Release-ComObject $mt
    }
    catch {
        $multiThreaded = [ordered]@{ error = $_.Exception.Message }
    }
    return [ordered]@{
        version                    = [string]$Excel.Version
        build                      = [string]$Excel.Build
        calculation                = [int]$Excel.Calculation
        iteration                  = [bool]$Excel.Iteration
        max_iterations             = [int]$Excel.MaxIterations
        max_change                 = [double]$Excel.MaxChange
        multi_threaded_calculation = $multiThreaded
    }
}

function New-ExcelSession {
    param(
        [string]$Path,
        [int]$Iterations,
        [double]$Change,
        [bool]$EnableMultiThreaded
    )
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
    $openWatch = [System.Diagnostics.Stopwatch]::StartNew()
    $workbook = $excel.Workbooks.Open($Path)
    $openWatch.Stop()
    $excel.Calculation = -4135
    $excel.Iteration = $true
    $excel.MaxIterations = $Iterations
    $excel.MaxChange = $Change
    try { $excel.MultiThreadedCalculation.Enabled = $EnableMultiThreaded } catch {}
    return [ordered]@{
        excel    = $excel
        workbook = $workbook
        open_ms  = [math]::Round($openWatch.Elapsed.TotalMilliseconds, 3)
    }
}

function Get-CellValueText {
    param([object]$Workbook, [string]$Sheet, [string]$Address)
    $worksheet = $Workbook.Worksheets.Item($Sheet)
    $cell = $worksheet.Range($Address)
    try {
        return [ordered]@{
            value2  = $cell.Value2
            text    = [string]$cell.Text
            formula = try { [string]$cell.Formula2 } catch { "" }
        }
    }
    finally {
        Release-ComObject $cell
        Release-ComObject $worksheet
    }
}

function Get-ObservationSnapshot {
    param([object]$Workbook)
    $snapshot = [ordered]@{}
    foreach ($target in $ObservationTargets) {
        $separator = $target.LastIndexOf("!")
        if ($separator -le 0) { continue }
        $sheet = $target.Substring(0, $separator)
        $address = $target.Substring($separator + 1)
        try {
            $snapshot[$target] = Get-CellValueText -Workbook $Workbook -Sheet $sheet -Address $address
        }
        catch {
            $snapshot[$target] = [ordered]@{ error = $_.Exception.Message }
        }
    }
    return $snapshot
}

function Add-ObservationSnapshot {
    param([object]$Measurement, [object]$Workbook)
    $Measurement.Add("target_snapshot", (Get-ObservationSnapshot -Workbook $Workbook))
    return $Measurement
}

function Invoke-Calculation {
    param(
        [object]$Excel,
        [string]$Method
    )
    $measurement = Measure-Action {
        switch ($Method) {
            "calculate" { $Excel.Calculate() }
            "calculate_full" { $Excel.CalculateFull() }
            "calculate_full_rebuild" { $Excel.CalculateFullRebuild() }
            default { throw "Unsupported calculation method: $Method" }
        }
        Wait-Calculation $Excel
    }
    $measurement.method = $Method
    return $measurement
}

function Set-CellValue {
    param([object]$Workbook, [string]$Sheet, [string]$Address, [object]$Value)
    $worksheet = $Workbook.Worksheets.Item($Sheet)
    $cell = $worksheet.Range($Address)
    try { $cell.Value2 = [double]$Value }
    finally {
        Release-ComObject $cell
        Release-ComObject $worksheet
    }
}

function Invoke-WarmScenario {
    param(
        [object]$Excel,
        [object]$Workbook,
        [int]$Iterations,
        [double]$Change
    )
    $initial = Add-ObservationSnapshot -Measurement (Invoke-Calculation -Excel $Excel -Method "calculate") -Workbook $Workbook
    Set-CellValue -Workbook $Workbook -Sheet $InputSheet -Address $InputAddress -Value $InputValue
    $f7 = Add-ObservationSnapshot -Measurement (Invoke-Calculation -Excel $Excel -Method "calculate") -Workbook $Workbook
    Set-CellValue -Workbook $Workbook -Sheet $InputSheet -Address $InputAddress -Value $InputValue
    $sameValue = Add-ObservationSnapshot -Measurement (Invoke-Calculation -Excel $Excel -Method "calculate") -Workbook $Workbook
    Set-CellValue -Workbook $Workbook -Sheet $InputSheet -Address $InputAddress -Value ($InputValue + 1)
    $changedAgain = Add-ObservationSnapshot -Measurement (Invoke-Calculation -Excel $Excel -Method "calculate") -Workbook $Workbook
    Set-CellValue -Workbook $Workbook -Sheet $InputSheet -Address $InputAddress -Value $InputValue
    $changedBack = Add-ObservationSnapshot -Measurement (Invoke-Calculation -Excel $Excel -Method "calculate") -Workbook $Workbook
    Set-CellValue -Workbook $Workbook -Sheet $UnrelatedSheet -Address $UnrelatedAddress -Value 1
    $unrelated = Add-ObservationSnapshot -Measurement (Invoke-Calculation -Excel $Excel -Method "calculate") -Workbook $Workbook
    $full = Add-ObservationSnapshot -Measurement (Invoke-Calculation -Excel $Excel -Method "calculate_full") -Workbook $Workbook
    $fullRebuild = Add-ObservationSnapshot -Measurement (Invoke-Calculation -Excel $Excel -Method "calculate_full_rebuild") -Workbook $Workbook
    return [ordered]@{
        initial_calculate = $initial
        f7_edit           = $f7
        same_value_write  = $sameValue
        changed_again     = $changedAgain
        changed_back      = $changedBack
        unrelated_edit    = $unrelated
        full              = $full
        full_rebuild      = $fullRebuild
        input_after       = Get-CellValueText -Workbook $Workbook -Sheet $InputSheet -Address $InputAddress
    }
}

function Invoke-ColdScenario {
    param(
        [int]$Iterations,
        [double]$Change,
        [string]$Method
    )
    $session = New-ExcelSession -Path $WorkbookPath -Iterations $Iterations -Change $Change -EnableMultiThreaded $MultiThreaded
    $excel = $session.excel
    $workbook = $session.workbook
    try {
        $calculation = Add-ObservationSnapshot -Measurement (Invoke-Calculation -Excel $excel -Method $Method) -Workbook $workbook
        Set-CellValue -Workbook $workbook -Sheet $InputSheet -Address $InputAddress -Value $InputValue
        $edit = Add-ObservationSnapshot -Measurement (Invoke-Calculation -Excel $excel -Method "calculate") -Workbook $workbook
        return [ordered]@{
            open_ms     = $session.open_ms
            calculation = $calculation
            f7_edit     = $edit
            settings    = Get-ExcelSettings -Excel $excel
        }
    }
    finally {
        try { $workbook.Close($false) } catch {}
        try { $excel.Quit() } catch {}
        Release-ComObject $workbook
        Release-ComObject $excel
        [GC]::Collect()
        [GC]::WaitForPendingFinalizers()
    }
}

$resolvedPath = (Resolve-Path $WorkbookPath).Path
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $OutputPath = Join-Path (Split-Path -Parent $resolvedPath) "excel-recalc-measurement.json"
}

$warmResults = @()
$coldResults = @()
$settings = $null
for ($iterationIndex = 0; $iterationIndex -lt $MaxIterations.Count; $iterationIndex++) {
    $iterationLimit = $MaxIterations[$iterationIndex]

    $session = New-ExcelSession -Path $resolvedPath -Iterations $iterationLimit -Change $MaxChange -EnableMultiThreaded $MultiThreaded
    $excel = $session.excel
    $workbook = $session.workbook
    try {
        if ($null -eq $settings) {
            $settings = Get-ExcelSettings -Excel $excel
        }
        for ($run = 1; $run -le $Runs; $run++) {
            $warm = Invoke-WarmScenario -Excel $excel -Workbook $workbook -Iterations $iterationLimit -Change $MaxChange
            $warmResults += [ordered]@{
                iteration_limit = $iterationLimit
                run             = $run
                open_ms         = $session.open_ms
                settings        = $settings
                measurements    = $warm
            }

            if ($run -lt $Runs) {
                $workbook.Close($false)
                Release-ComObject $workbook
                $workbook = $null
                $reopened = New-ExcelSession -Path $resolvedPath -Iterations $iterationLimit -Change $MaxChange -EnableMultiThreaded $MultiThreaded
                $excel = $reopened.excel
                $workbook = $reopened.workbook
                $session = $reopened
            }
        }
    }
    finally {
        try { $workbook.Close($false) } catch {}
        try { $excel.Quit() } catch {}
        Release-ComObject $workbook
        Release-ComObject $excel
        [GC]::Collect()
        [GC]::WaitForPendingFinalizers()
    }

    for ($run = 1; $run -le $Runs; $run++) {
        $cold = Invoke-ColdScenario -Iterations $iterationLimit -Change $MaxChange -Method "calculate"
        $coldResults += [ordered]@{
            iteration_limit = $iterationLimit
            run             = $run
            measurements    = $cold
        }
    }
}

$report = [ordered]@{
    schema                   = "formualizer.excel-recalc-investigation/v1"
    generated_at_utc         = [DateTime]::UtcNow.ToString("o")
    workbook                 = $resolvedPath
    input                    = [ordered]@{
        sheet   = $InputSheet
        address = $InputAddress
        value   = $InputValue
    }
    unrelated_input          = [ordered]@{
        sheet   = $UnrelatedSheet
        address = $UnrelatedAddress
        value   = 1
    }
    max_change               = $MaxChange
    multi_threaded           = $MultiThreaded
    runs_per_iteration_limit = $Runs
    settings                 = $settings
    warm_results             = $warmResults
    cold_results             = $coldResults
    notes                    = @(
        "Warm results reuse one Excel process/workbook for each iteration limit, reopening between runs.",
        "Cold results use a new Excel.Application and workbook for every run.",
        "The workbook is never saved by this measurement script.",
        "Application.Calculate, CalculateFull, and CalculateFullRebuild are timed separately.",
        "Excel calculation state waits for xlCalculating to finish; xlPending is a valid state after a capped iterative Calculate call."
    )
}

$json = $report | ConvertTo-Json -Depth 30
[System.IO.File]::WriteAllText($OutputPath, $json + [Environment]::NewLine, [System.Text.UTF8Encoding]::new($false))
Write-Output "Generated $OutputPath"
