param(
    [Parameter(Mandatory = $true)]
    [string]$WorkbookPath,
    [Parameter(Mandatory = $true)]
    [string]$OutputPath,
    [string]$InputSheet = "Inputs",
    [string]$InputAddress = "F7",
    [double]$InputValue = 300,
    [int]$MaxIterations = 100,
    [double]$MaxChange = 0.001,
    [string[]]$TrackedAddresses = @(
        "CashFlow Engine!Z33",
        "CashFlow Engine!Z84",
        "CashFlow Engine!Z85",
        "CashFlow Engine!Z86",
        "CashFlow Engine!Z93",
        "CashFlow Engine!Z94",
        "CashFlow Engine!Z95",
        "CashFlow Engine!Z96",
        "CashFlow Engine!Z97",
        "CashFlow Engine!Z109",
        "CashFlow Engine!Z110"
    )
)

$ErrorActionPreference = "Stop"

function Release-ComObject {
    param([object]$Object)
    if ($null -ne $Object -and [System.Runtime.InteropServices.Marshal]::IsComObject($Object)) {
        [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($Object)
    }
}

function Convert-ToColumnName {
    param([int]$Column)
    $name = ""
    while ($Column -gt 0) {
        $remainder = ($Column - 1) % 26
        $name = [char](65 + $remainder) + $name
        $Column = [math]::Floor(($Column - 1) / 26)
    }
    return $name
}

function Get-ArrayElement {
    param(
        [object]$Array,
        [int]$Row,
        [int]$Column,
        [int]$RowCount,
        [int]$ColumnCount
    )
    if ($RowCount -eq 1 -and $ColumnCount -eq 1) {
        return $Array
    }
    return $Array.GetValue($Row, $Column)
}

function Convert-ValueCanonical {
    param([object]$Value)
    if ($null -eq $Value) {
        return "null"
    }
    if ($Value -is [bool]) {
        return "bool:" + ([bool]$Value).ToString().ToLowerInvariant()
    }
    if ($Value -is [double] -or $Value -is [single] -or $Value -is [decimal] -or
        $Value -is [int] -or $Value -is [long]) {
        return "number:" + ([double]$Value).ToString("R", [Globalization.CultureInfo]::InvariantCulture)
    }
    if ($Value -is [datetime]) {
        return "datetime:" + ([datetime]$Value).ToString("o", [Globalization.CultureInfo]::InvariantCulture)
    }
    return "text:" + [string]$Value
}

function Get-FormulaSnapshot {
    param([object]$Workbook)
    $snapshot = @{}
    foreach ($worksheet in $Workbook.Worksheets) {
        $used = $null
        $formulaCells = $null
        try {
            $used = $worksheet.UsedRange
            try {
                $formulaCells = $used.SpecialCells(-4123)
            }
            catch {
                continue
            }
            foreach ($area in $formulaCells.Areas) {
                $rowCount = [int]$area.Rows.Count
                $columnCount = [int]$area.Columns.Count
                $formulas = $area.Formula2
                $values = $area.Value2
                for ($row = 1; $row -le $rowCount; $row++) {
                    for ($column = 1; $column -le $columnCount; $column++) {
                        $formula = Get-ArrayElement -Array $formulas -Row $row -Column $column -RowCount $rowCount -ColumnCount $columnCount
                        if ($null -eq $formula -or [string]::IsNullOrWhiteSpace([string]$formula)) {
                            continue
                        }
                        $absoluteRow = [int]$area.Row + $row - 1
                        $absoluteColumn = [int]$area.Column + $column - 1
                        $address = "{0}!{1}{2}" -f $worksheet.Name, (Convert-ToColumnName $absoluteColumn), $absoluteRow
                        $value = Get-ArrayElement -Array $values -Row $row -Column $column -RowCount $rowCount -ColumnCount $columnCount
                        $snapshot[$address] = [ordered]@{
                            formula = [string]$formula
                            value   = Convert-ValueCanonical $value
                        }
                    }
                }
                Release-ComObject $formulas
                Release-ComObject $values
                Release-ComObject $area
            }
        }
        finally {
            Release-ComObject $formulaCells
            Release-ComObject $used
            Release-ComObject $worksheet
        }
    }
    return $snapshot
}

function Get-TrackedValues {
    param(
        [hashtable]$Snapshot,
        [string[]]$Addresses
    )
    $tracked = [ordered]@{}
    foreach ($address in $Addresses) {
        if ($Snapshot.ContainsKey($address)) {
            $tracked[$address] = $Snapshot[$address].value
        }
        else {
            $tracked[$address] = "<missing>"
        }
    }
    return $tracked
}

function Get-SnapshotFingerprint {
    param([hashtable]$Snapshot)
    $builder = [System.Text.StringBuilder]::new()
    foreach ($key in ($Snapshot.Keys | Sort-Object)) {
        [void]$builder.Append($key)
        [void]$builder.Append("|")
        [void]$builder.Append($Snapshot[$key].formula)
        [void]$builder.Append("|")
        [void]$builder.Append($Snapshot[$key].value)
        [void]$builder.Append("`n")
    }
    $bytes = [System.Text.UTF8Encoding]::new($false).GetBytes($builder.ToString())
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
        return ([System.BitConverter]::ToString($sha256.ComputeHash($bytes))).Replace("-", "").ToLowerInvariant()
    }
    finally {
        $sha256.Dispose()
    }
}

function Convert-CanonicalNumeric {
    param([string]$Canonical)
    if ($Canonical.StartsWith("number:")) {
        $number = 0.0
        if ([double]::TryParse(
                $Canonical.Substring(7),
                [Globalization.NumberStyles]::Float,
                [Globalization.CultureInfo]::InvariantCulture,
                [ref]$number)) {
            return $number
        }
    }
    return $null
}

function Compare-Snapshots {
    param(
        [hashtable]$Before,
        [hashtable]$After
    )
    $changed = New-Object System.Collections.Generic.List[object]
    $keys = @($Before.Keys + $After.Keys | Sort-Object -Unique)
    foreach ($key in $keys) {
        $beforeCell = $Before[$key]
        $afterCell = $After[$key]
        $beforeValue = if ($null -eq $beforeCell) { "<missing>" } else { [string]$beforeCell.value }
        $afterValue = if ($null -eq $afterCell) { "<missing>" } else { [string]$afterCell.value }
        if ($beforeValue -eq $afterValue) {
            continue
        }
        $beforeNumber = Convert-CanonicalNumeric $beforeValue
        $afterNumber = Convert-CanonicalNumeric $afterValue
        $delta = $null
        if ($null -ne $beforeNumber -and $null -ne $afterNumber) {
            $delta = [math]::Abs($afterNumber - $beforeNumber)
        }
        $changed.Add([ordered]@{
                address   = $key
                before    = $beforeValue
                after     = $afterValue
                abs_delta = $delta
            })
    }
    return $changed
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

function Wait-Calculation {
    param([object]$Excel)
    while ([int]$Excel.CalculationState -eq 1) {
        Start-Sleep -Milliseconds 10
    }
}

function Invoke-Calculate {
    param([object]$Excel)
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $Excel.Calculate()
    Wait-Calculation $Excel
    $watch.Stop()
    return [math]::Round($watch.Elapsed.TotalMilliseconds, 3)
}

function Set-InputValue {
    param([object]$Workbook)
    $worksheet = $null
    $cell = $null
    try {
        $worksheet = $Workbook.Worksheets.Item($InputSheet)
        $cell = $worksheet.Range($InputAddress)
        $cell.Value2 = [double]$InputValue
    }
    finally {
        Release-ComObject $cell
        Release-ComObject $worksheet
    }
}

$excel = New-Object -ComObject Excel.Application
$excel.Visible = $false
$excel.DisplayAlerts = $false
try { $excel.AutomationSecurity = 3 } catch {}
try { $excel.AskToUpdateLinks = $false } catch {}
$workbook = $null
$bootstrap = $null
try {
    $bootstrap = $excel.Workbooks.Add()
    try {
        $excel.Calculation = -4135
        $excel.Iteration = $true
        $excel.MaxIterations = $MaxIterations
        $excel.MaxChange = $MaxChange
    }
    finally {
        try { $bootstrap.Close($false) } catch {}
        Release-ComObject $bootstrap
        $bootstrap = $null
    }
    $openWatch = [System.Diagnostics.Stopwatch]::StartNew()
    $workbook = $excel.Workbooks.Open($WorkbookPath, 0, $false)
    $openWatch.Stop()
    $excel.Calculation = -4135
    $excel.Iteration = $true
    $excel.MaxIterations = $MaxIterations
    $excel.MaxChange = $MaxChange
    try { $excel.MultiThreadedCalculation.Enabled = $true } catch {}

    Set-InputValue $workbook
    $seedMs = Invoke-Calculate $excel
    $previous = Get-FormulaSnapshot $workbook
    $seedTrackedValues = Get-TrackedValues -Snapshot $previous -Addresses $TrackedAddresses
    $steps = New-Object System.Collections.Generic.List[object]
    for ($step = 1; $step -le 5; $step++) {
        $elapsedMs = Invoke-Calculate $excel
        $current = Get-FormulaSnapshot $workbook
        $changed = Compare-Snapshots -Before $previous -After $current
        $numericDeltas = @($changed | Where-Object { $null -ne $_.abs_delta } | ForEach-Object { [double]$_.abs_delta })
        $maxDelta = if ($numericDeltas.Count -eq 0) { 0.0 } else { ($numericDeltas | Measure-Object -Maximum).Maximum }
        $steps.Add([ordered]@{
                calculate                  = $step
                wall_ms                    = $elapsedMs
                formula_count              = $current.Count
                formula_output_fingerprint = Get-SnapshotFingerprint $current
                tracked_values             = Get-TrackedValues -Snapshot $current -Addresses $TrackedAddresses
                changed_formula_count      = $changed.Count
                max_abs_numeric_delta      = [double]$maxDelta
                changed_formulas           = $changed
            })
        $previous = $current
    }
    $result = [ordered]@{
        schema               = "formualizer.excel-repeated-noop/v1"
        workbook             = [System.IO.Path]::GetFileName($WorkbookPath)
        input                = [ordered]@{ sheet = $InputSheet; address = $InputAddress; value = $InputValue }
        open_ms              = [math]::Round($openWatch.Elapsed.TotalMilliseconds, 3)
        f7_seed_calculate_ms = $seedMs
        seed_tracked_values  = $seedTrackedValues
        seed_formula_outputs = $previous
        tracked_addresses    = $TrackedAddresses
        settings             = Get-ExcelSettings $excel
        steps                = $steps
    }
    $result | ConvertTo-Json -Depth 12 | Set-Content -Path $OutputPath -Encoding UTF8
}
finally {
    if ($null -ne $workbook) {
        try { $workbook.Close($false) } catch {}
    }
    try { $excel.Quit() } catch {}
    Release-ComObject $workbook
    Release-ComObject $bootstrap
    Release-ComObject $excel
}
