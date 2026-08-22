param(
    [Parameter(Mandatory = $true)]
    [string[]]$CasePath
)

$ErrorActionPreference = "Stop"

function Release-ComObject {
    param([object]$Object)
    if ($null -ne $Object -and [System.Runtime.InteropServices.Marshal]::IsComObject($Object)) {
        [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($Object)
    }
}

function Set-OracleCell {
    param(
        [object]$Worksheet,
        [object]$Spec
    )

    $cell = $Worksheet.Range([string]$Spec.address)
    try {
        switch ([string]$Spec.kind) {
            "formula" { $cell.Formula2 = [string]$Spec.value }
            "formula_iie" { $cell.Formula = [string]$Spec.value }
            "number" { $cell.Value2 = [double]$Spec.value }
            "text" { $cell.Value2 = [string]$Spec.value }
            "boolean" { $cell.Value2 = [bool]$Spec.value }
            "empty" { [void]$cell.ClearContents() }
            default { throw "Unsupported cell kind '$($Spec.kind)' at $($Spec.address)" }
        }
    }
    finally {
        Release-ComObject $cell
    }
}

function Get-OracleResult {
    param(
        [object]$Workbook,
        [string]$Target
    )

    $separator = $Target.LastIndexOf("!")
    if ($separator -le 0 -or $separator -eq $Target.Length - 1) {
        throw "Target must be Sheet!A1: $Target"
    }
    $sheetName = $Target.Substring(0, $separator)
    if ($sheetName.StartsWith("'") -and $sheetName.EndsWith("'")) {
        $sheetName = $sheetName.Substring(1, $sheetName.Length - 2).Replace("''", "'")
    }
    $address = $Target.Substring($separator + 1).Replace("$", "")
    $worksheet = $Workbook.Worksheets.Item($sheetName)
    $cell = $worksheet.Range($address)
    try {
        $value = $cell.Value2
        $text = [string]$cell.Text
        $errorLabels = @(
            "#NULL!", "#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#NUM!", "#N/A",
            "#GETTING_DATA", "#SPILL!", "#CALC!", "#FIELD!", "#BLOCKED!", "#UNKNOWN!"
        )
        $kind = if ($errorLabels -contains $text.ToUpperInvariant()) {
            "error"
        }
        elseif ($null -eq $value -or $value -eq "") {
            "empty"
        }
        elseif ($value -is [bool]) {
            "boolean"
        }
        elseif ($value -is [byte] -or $value -is [int16] -or $value -is [int32] -or
            $value -is [int64] -or $value -is [single] -or $value -is [double] -or
            $value -is [decimal]) {
            "number"
        }
        else {
            "text"
        }

        $formula = try { [string]$cell.Formula } catch { "" }
        $formula2 = try { [string]$cell.Formula2 } catch { $formula }
        [ordered]@{
            kind          = $kind
            value         = if ($kind -eq "empty" -or $kind -eq "error") { $null } else { $value }
            error         = if ($kind -eq "error") { $text.ToUpperInvariant() } else { $null }
            text          = $text
            formula       = $formula
            formula2      = $formula2
            number_format = [string]$cell.NumberFormat
        }
    }
    finally {
        Release-ComObject $cell
        Release-ComObject $worksheet
    }
}

function Invoke-ExcelOracleCase {
    param([string]$InputCasePath)

    $resolvedCase = (Resolve-Path $InputCasePath).Path
    $caseDirectory = Split-Path -Parent $resolvedCase
    $case = Get-Content -Raw $resolvedCase | ConvertFrom-Json
    if ($case.schema -ne "formualizer.excel-oracle.case/v1") {
        throw "Unsupported oracle case schema '$($case.schema)' in $resolvedCase"
    }

    $workbookFile = [string]$case.workbook.file
    if ([System.IO.Path]::GetFileName($workbookFile) -ne $workbookFile -or
        [System.IO.Path]::GetExtension($workbookFile) -ne ".xlsx") {
        throw "workbook.file must be an XLSX basename: $workbookFile"
    }
    $workbookPath = Join-Path $caseDirectory $workbookFile
    $snapshotPath = Join-Path $caseDirectory "expected.excel.json"
    if (Test-Path $workbookPath) {
        Remove-Item $workbookPath -Force
    }

    $excel = $null
    $workbook = $null
    $oldCalculation = $null
    $oldIteration = $null
    $oldMaxIterations = $null
    $oldMaxChange = $null
    try {
        $excel = New-Object -ComObject Excel.Application
        $excel.Visible = $false
        $excel.DisplayAlerts = $false
        $oldCalculation = $excel.Calculation
        $oldIteration = $excel.Iteration
        $oldMaxIterations = $excel.MaxIterations
        $oldMaxChange = $excel.MaxChange
        $workbook = $excel.Workbooks.Add()
        $workbook.RemovePersonalInformation = $true
        $excel.Calculation = -4135
        $excel.Iteration = [bool]$case.workbook.calculation.iterate
        $excel.MaxIterations = [int]$case.workbook.calculation.max_iterations
        $excel.MaxChange = [double]$case.workbook.calculation.max_change
        $workbook.Date1904 = ([string]$case.workbook.date_system -eq "1904")

        while ($workbook.Worksheets.Count -gt 1) {
            $extra = $workbook.Worksheets.Item($workbook.Worksheets.Count)
            try { $extra.Delete() } finally { Release-ComObject $extra }
        }

        for ($sheetIndex = 0; $sheetIndex -lt $case.workbook.sheets.Count; $sheetIndex++) {
            $sheetSpec = $case.workbook.sheets[$sheetIndex]
            if ($sheetIndex -eq 0) {
                $worksheet = $workbook.Worksheets.Item(1)
            }
            else {
                $last = $workbook.Worksheets.Item($workbook.Worksheets.Count)
                try { $worksheet = $workbook.Worksheets.Add([System.Type]::Missing, $last) }
                finally { Release-ComObject $last }
            }
            try {
                $worksheet.Name = [string]$sheetSpec.name
                foreach ($cellSpec in $sheetSpec.cells) {
                    Set-OracleCell -Worksheet $worksheet -Spec $cellSpec
                }
            }
            finally {
                Release-ComObject $worksheet
            }
        }

        $workbook.SaveAs($workbookPath, 51)
        $excel.CalculateFullRebuild()
        while ($excel.CalculationState -ne 0) {
            Start-Sleep -Milliseconds 25
        }
        $workbook.Save()

        $results = [ordered]@{}
        foreach ($target in $case.targets) {
            $results[[string]$target] = Get-OracleResult -Workbook $workbook -Target ([string]$target)
        }

        $excelVersion = [string]$excel.Version
        $excelExecutable = Join-Path ([string]$excel.Path) "EXCEL.EXE"
        $excelFileVersion = if (Test-Path $excelExecutable) {
            (Get-Item $excelExecutable).VersionInfo.FileVersion
        }
        else {
            $null
        }
        $workbook.Close($true)
        Release-ComObject $workbook
        $workbook = $null

        $snapshot = [ordered]@{
            schema     = "formualizer.excel-oracle.snapshot/v1"
            case_id    = [string]$case.id
            provenance = [ordered]@{
                generated_at_utc   = [DateTime]::UtcNow.ToString("o")
                generator          = "tools/excel-oracle/recalculate_excel.ps1"
                excel_version      = $excelVersion
                excel_file_version = $excelFileVersion
                excel_executable   = $excelExecutable
                culture            = [System.Globalization.CultureInfo]::CurrentCulture.Name
                case_sha256        = (Get-FileHash -Algorithm SHA256 $resolvedCase).Hash.ToLowerInvariant()
                workbook_sha256    = (Get-FileHash -Algorithm SHA256 $workbookPath).Hash.ToLowerInvariant()
                date_system        = [string]$case.workbook.date_system
                calculation        = $case.workbook.calculation
            }
            results    = $results
        }
        $snapshotJson = $snapshot | ConvertTo-Json -Depth 20
        [System.IO.File]::WriteAllText(
            $snapshotPath,
            $snapshotJson + [Environment]::NewLine,
            [System.Text.UTF8Encoding]::new($false)
        )
        Write-Output "Generated $workbookPath"
        Write-Output "Generated $snapshotPath"
    }
    finally {
        if ($null -ne $workbook) {
            try { $workbook.Close($false) } catch {}
        }
        if ($null -ne $excel) {
            try {
                if ($null -ne $oldCalculation) { $excel.Calculation = $oldCalculation }
                if ($null -ne $oldIteration) { $excel.Iteration = $oldIteration }
                if ($null -ne $oldMaxIterations) { $excel.MaxIterations = $oldMaxIterations }
                if ($null -ne $oldMaxChange) { $excel.MaxChange = $oldMaxChange }
            }
            catch {}
            try { $excel.Quit() } catch {}
        }
        Release-ComObject $workbook
        Release-ComObject $excel
        [GC]::Collect()
        [GC]::WaitForPendingFinalizers()
        [GC]::Collect()
        [GC]::WaitForPendingFinalizers()
    }
}

foreach ($path in $CasePath) {
    Invoke-ExcelOracleCase -InputCasePath $path
}
