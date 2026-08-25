$ErrorActionPreference = "Stop"
$excel = New-Object -ComObject Excel.Application
$excel.Visible = $false
$excel.DisplayAlerts = $false
$workbook = $null
try {
    $workbook = $excel.Workbooks.Add()
    $worksheet = $workbook.Worksheets.Item(1)
    try {
        $worksheet.Range("A1").Formula2 = "=B1+1"
        $worksheet.Range("B1").Formula2 = "=A1/2"
        $results = @()
        foreach ($iteration in @($true, $false)) {
            $excel.Calculation = -4135
            $excel.Iteration = $iteration
            $excel.MaxIterations = 100
            $excel.MaxChange = 0.001
            $excel.CalculateFullRebuild()
            $state = [int]$excel.CalculationState
            $circular = $null
            $errorText = $null
            try { $circular = $worksheet.CircularReference } catch { $errorText = $_.Exception.Message }
            $results += [ordered]@{
                iteration_enabled = $iteration
                calculation_state = $state
                circular = if ($null -eq $circular) { $null } else { [string]$circular.Address($false, $false) }
                circular_error = $errorText
                a1_text = [string]$worksheet.Range("A1").Text
                b1_text = [string]$worksheet.Range("B1").Text
            }
            if ($null -ne $circular) { [System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($circular) }
        }
        $results | ConvertTo-Json -Depth 10
    }
    finally { [System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($worksheet) }
}
finally {
    if ($null -ne $workbook) { try { $workbook.Close($false) } catch {}; [System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($workbook) }
    try { $excel.Quit() } catch {}
    [System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($excel)
}
