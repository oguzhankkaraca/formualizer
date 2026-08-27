param(
    [string]$WorkbookPath = "C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-08_21_A.xlsx"
)

$ErrorActionPreference = "Stop"

function Release-ComObject {
    param([object]$Object)
    if ($null -ne $Object -and [System.Runtime.InteropServices.Marshal]::IsComObject($Object)) {
        [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($Object)
    }
}

function Get-CellInfo {
    param([object]$Worksheet, [string]$Address)
    $cell = $null
    try {
        $cell = $Worksheet.Range($Address)
        $value2 = $null
        $text = $null
        try { $value2 = $cell.Value2 } catch { $value2 = "<read-error>" }
        try { $text = [string]$cell.Text } catch { $text = "<read-error>" }
        $value2Output = if ($null -eq $value2) { $null } else { [string]$value2 }
        return [ordered]@{
            address = $Address
            value2  = $value2Output
            text    = $text
        }
    }
    finally {
        Release-ComObject $cell
    }
}

function Get-NameInfo {
    param([object]$Workbook, [string]$Name)
    $nameObject = $null
    $range = $null
    try {
        $nameObject = $Workbook.Names.Item($Name)
        $refersTo = [string]$nameObject.RefersTo
        $address = $null
        try {
            $range = $nameObject.RefersToRange
            $address = ([string]$range.Worksheet.Name) + "!" + ([string]$range.Address($false, $false))
        }
        catch {
            $address = "<not-range>"
        }
        return [ordered]@{
            name      = $Name
            refers_to = $refersTo
            address   = $address
        }
    }
    finally {
        Release-ComObject $range
        Release-ComObject $nameObject
    }
}

if (-not (Test-Path -LiteralPath $WorkbookPath)) {
    throw "Workbook not found: $WorkbookPath"
}

$disposableCopy = Join-Path $env:TEMP ("formualizer-engine-v2-j11-" + [guid]::NewGuid().ToString() + ".xlsx")
Copy-Item -LiteralPath $WorkbookPath -Destination $disposableCopy -Force
$excel = $null
$workbook = $null
$engineSheet = $null
$inputsSheet = $null
$cashFlowInputsSheet = $null

try {
    $excel = New-Object -ComObject Excel.Application
    $excel.Visible = $false
    $excel.DisplayAlerts = $false
    try { $excel.AutomationSecurity = 3 } catch {}
    try { $excel.AskToUpdateLinks = $false } catch {}

    $workbook = $excel.Workbooks.Open($disposableCopy, 0, $false)
    $excel.Calculation = -4135
    $excel.Iteration = $true
    $excel.MaxIterations = 100
    $excel.MaxChange = 0.001

    $inputsSheet = $workbook.Worksheets.Item("Inputs")
    $inputsSheet.Range("F7").Value2 = 300
    $excel.CalculateFullRebuild()
    while ([int]$excel.CalculationState -eq 1) {
        Start-Sleep -Milliseconds 10
    }

    $engineSheet = $workbook.Worksheets.Item("CashFlow Engine")
    $cashFlowInputsSheet = $workbook.Worksheets.Item("CashFlow Inputs")
    $c11 = Get-CellInfo $engineSheet "C11"
    $j6 = Get-CellInfo $engineSheet "J6"
    $sourceInfo = Get-NameInfo $workbook "Cash_Flow_Inputs"
    $rowInfo = Get-NameInfo $workbook "Cash_Flow_Inputs_R"
    $columnInfo = Get-NameInfo $workbook "Cash_Flow_Inputs_C"
    $j24Info = Get-CellInfo $cashFlowInputsSheet "J24"
    $k40Info = Get-CellInfo $engineSheet "K40"
    $i74Info = Get-CellInfo $engineSheet "I74"
    $k74Info = Get-CellInfo $engineSheet "K74"
    $minK29K112 = $null
    $minusTwelve = $null
    $edateJ23 = $null
    try { $minK29K112 = $engineSheet.Evaluate("=MIN(K29:K112)") } catch { $minK29K112 = "<evaluate-error>" }
    try { $minusTwelve = ([double]$minK29K112) - 12 } catch { $minusTwelve = "<coercion-error>" }
    try { $edateJ23 = $cashFlowInputsSheet.Evaluate("=EDATE(J24,MIN('CashFlow Engine'!K29:K112)-12)") } catch { $edateJ23 = "<evaluate-error>" }

    $sourceName = $null
    $rowName = $null
    $columnName = $null
    $sourceRange = $null
    $rowRange = $null
    $columnRange = $null
    try {
        $sourceName = $workbook.Names.Item("Cash_Flow_Inputs")
        $rowName = $workbook.Names.Item("Cash_Flow_Inputs_R")
        $columnName = $workbook.Names.Item("Cash_Flow_Inputs_C")
        $sourceRange = $sourceName.RefersToRange
        $rowRange = $rowName.RefersToRange
        $columnRange = $columnName.RefersToRange
        $rowMatch = $excel.WorksheetFunction.Match($c11.value2, $rowRange, 0)
        $columnMatch = $excel.WorksheetFunction.Match($j6.value2, $columnRange, 0)
    }
    finally {
        Release-ComObject $sourceRange
        Release-ComObject $rowRange
        Release-ComObject $columnRange
        Release-ComObject $sourceName
        Release-ComObject $rowName
        Release-ComObject $columnName
    }

    $referenceFormula = '=CELL("address",INDEX(Cash_Flow_Inputs,MATCH($C11,Cash_Flow_Inputs_R,0),MATCH($J$6,Cash_Flow_Inputs_C,0)))'
    $selectedRaw = [string]$engineSheet.Evaluate($referenceFormula)
    $selectedCell = ($selectedRaw -replace '^.*\]', '').Replace("'", "").Replace("$", "")
    if ($selectedCell -notmatch "!") {
        $selectedCell = ([string]$cashFlowInputsSheet.Name) + "!" + $selectedCell
    }

    $output = [ordered]@{
        workbook                      = (Resolve-Path $WorkbookPath).Path
        disposable_copy               = $disposableCopy
        input                         = [ordered]@{ sheet = "Inputs"; address = "F7"; value = 300 }
        c11                           = $c11
        j6                            = $j6
        j24                           = $j24Info
        min_k29_k112                  = $minK29K112
        min_minus_12                  = $minusTwelve
        edate_j23                     = $edateJ23
        k40                           = $k40Info
        i74                           = $i74Info
        k74                           = $k74Info
        cash_flow_inputs              = $sourceInfo
        cash_flow_inputs_r            = $rowInfo
        cash_flow_inputs_c            = $columnInfo
        match_c11                     = $rowMatch
        match_j6                      = $columnMatch
        index_selected_worksheet_cell = $selectedCell
        index_selected_raw            = $selectedRaw
        j11                           = Get-CellInfo $engineSheet "J11"
        i65                           = Get-CellInfo $engineSheet "I65"
        k65                           = Get-CellInfo $engineSheet "K65"
        cashflow_inputs_j23           = Get-CellInfo $cashFlowInputsSheet "J23"
        reference_formula             = $referenceFormula
    }
    $output | ConvertTo-Json -Depth 20
}
finally {
    Release-ComObject $engineSheet
    Release-ComObject $inputsSheet
    Release-ComObject $cashFlowInputsSheet
    if ($null -ne $workbook) {
        try { $workbook.Close($false) } catch {}
        Release-ComObject $workbook
    }
    if ($null -ne $excel) {
        try { $excel.Quit() } catch {}
        Release-ComObject $excel
    }
    [GC]::Collect()
    [GC]::WaitForPendingFinalizers()
    if (Test-Path -LiteralPath $disposableCopy) {
        Remove-Item -LiteralPath $disposableCopy -Force
    }
}
