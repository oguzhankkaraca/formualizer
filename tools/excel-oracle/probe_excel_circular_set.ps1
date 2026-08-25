param(
    [string]$WorkbookPath = "C:\Users\OXK0A0A\Downloads\Fossil_EstimatingTemplate_2026-08_21_A.xlsx",
    [string]$FormualizerBaselinePath = "C:\rust_engines\formualizer-v0.8.4\docs\issue-solutions\data\latest-upstream-heavy-baseline.json",
    [string]$FormualizerRuntimePath = "C:\rust_engines\formualizer-v0.8.4\docs\issue-solutions\data\fossil-runtime-live-scc-topology.json",
    [string]$OutputPath = "C:\rust_engines\formualizer-v0.8.4\docs\issue-solutions\data\excel-circular-set-oracle.json",
    [int]$TimingWarmRuns = 3
)

$ErrorActionPreference = "Stop"
$xlCalculationManual = -4135
$xlPasteValues = -4163

function Release-ComObject {
    param([object]$Object)
    if ($null -ne $Object -and [System.Runtime.InteropServices.Marshal]::IsComObject($Object)) {
        [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($Object)
    }
}

function Normalize-Address {
    param([string]$Address)
    $text = ([string]$Address).Trim()
    $bang = $text.LastIndexOf("!")
    if ($bang -lt 1) { return $text.ToUpperInvariant() }
    $sheet = $text.Substring(0, $bang).Trim("'").Replace("''", "'")
    $cell = $text.Substring($bang + 1).Replace("$", "")
    return ("{0}!{1}" -f $sheet, $cell).ToUpperInvariant()
}

function Convert-ComValue {
    param([object]$Value)
    if ($null -eq $Value) { return $null }
    if ($Value -is [System.Array]) {
        $items = @()
        foreach ($item in $Value) { $items += Convert-ComValue $item }
        return $items
    }
    try {
        if ($Value -is [System.IConvertible]) { return $Value.ToString() }
    }
    catch {}
    return [string]$Value
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

function Wait-ExcelCalculation {
    param([object]$Excel)
    $guard = 0
    while ([int]$Excel.CalculationState -eq 1 -and $guard -lt 36000) {
        Start-Sleep -Milliseconds 10
        $guard++
    }
    return [int]$Excel.CalculationState
}

function New-OracleSession {
    param(
        [string]$SourcePath,
        [string]$CopyPath,
        [bool]$Iteration,
        [int]$MaxIterations = 100,
        [double]$MaxChange = 0.001,
        [bool]$MultiThreaded = $true
    )
    Copy-Item -LiteralPath $SourcePath -Destination $CopyPath -Force
    $excel = New-Object -ComObject Excel.Application
    $excel.Visible = $false
    $excel.DisplayAlerts = $false
    try { $excel.AutomationSecurity = 3 } catch {}
    try { $excel.AskToUpdateLinks = $false } catch {}
    $beforeOpen = Get-ExcelSettings $excel
    $workbook = $null
    try {
        $workbook = $excel.Workbooks.Open($CopyPath, 0, $false)
        $afterOpen = Get-ExcelSettings $excel
        $excel.Calculation = $xlCalculationManual
        $excel.Iteration = $Iteration
        $excel.MaxIterations = $MaxIterations
        $excel.MaxChange = $MaxChange
        try { $excel.MultiThreadedCalculation.Enabled = $MultiThreaded } catch {}
        return [ordered]@{
            excel                = $excel
            workbook             = $workbook
            settings_before_open = $beforeOpen
            settings_after_open  = $afterOpen
            settings_configured  = Get-ExcelSettings $excel
        }
    }
    catch {
        if ($null -ne $workbook) { try { $workbook.Close($false) } catch {} }
        try { $excel.Quit() } catch {}
        Release-ComObject $workbook
        Release-ComObject $excel
        throw
    }
}

function Close-OracleSession {
    param([object]$Session)
    if ($null -eq $Session) { return }
    $workbook = $Session.workbook
    $excel = $Session.excel
    if ($null -ne $workbook) { try { $workbook.Close($false) } catch {} }
    if ($null -ne $excel) { try { $excel.Quit() } catch {} }
    Release-ComObject $workbook
    Release-ComObject $excel
    [GC]::Collect()
    [GC]::WaitForPendingFinalizers()
}

function Invoke-FullRebuild {
    param([object]$Excel)
    $state = $null
    $errorText = $null
    try {
        $Excel.CalculateFullRebuild()
        $state = Wait-ExcelCalculation $Excel
    }
    catch { $errorText = $_.Exception.Message }
    return [ordered]@{ calculation_state = $state; error = $errorText }
}

function Get-CellInfo {
    param([object]$Worksheet, [object]$Cell)
    $formula = $null
    $formula2 = $null
    $value2 = $null
    $text = $null
    try { $formula = [string]$Cell.Formula } catch {}
    try { $formula2 = [string]$Cell.Formula2 } catch { $formula2 = $formula }
    try { $value2 = Convert-ComValue $Cell.Value2 } catch {}
    try { $text = [string]$Cell.Text } catch {}
    return [ordered]@{
        sheet              = [string]$Worksheet.Name
        address            = [string]$Cell.Address($false, $false)
        normalized_address = Normalize-Address (([string]$Worksheet.Name) + "!" + ([string]$Cell.Address($false, $false)))
        formula            = $formula
        formula2           = $formula2
        value2             = $value2
        text               = $text
    }
}

function Get-CircularSeeds {
    param([object]$Workbook)
    $seeds = @()
    foreach ($worksheet in $Workbook.Worksheets) {
        $circular = $null
        try { $circular = $worksheet.CircularReference } catch {}
        if ($null -ne $circular) {
            $first = $null
            try { $first = $circular.Cells.Item(1, 1) } catch { $first = $circular }
            try {
                $info = Get-CellInfo -Worksheet $worksheet -Cell $first
                $info.circular_range_address = [string]$circular.Address($false, $false)
                $info.exposed_by = "Worksheet.CircularReference"
                $seeds += $info
            }
            finally {
                if ($first -ne $circular) { Release-ComObject $first }
                Release-ComObject $circular
            }
        }
        Release-ComObject $worksheet
    }
    return @($seeds)
}

function Get-AddressFromRange {
    param([object]$Range)
    $sheet = $null
    try { $sheet = $Range.Worksheet.Name } catch {}
    $address = $null
    try { $address = $Range.Address($false, $false) } catch { $address = [string]$Range }
    if ($null -eq $sheet) { return [ordered]@{ text = [string]$address } }
    return [ordered]@{
        sheet              = [string]$sheet
        address            = [string]$address
        normalized_address = Normalize-Address (([string]$sheet) + "!" + ([string]$address))
    }
}

function Trace-DependencyDirection {
    param(
        [object]$Excel,
        [object]$Workbook,
        [object]$Seed,
        [bool]$Forward
    )
    $result = [ordered]@{
        seed                 = $Seed.normalized_address
        direction            = if ($Forward) { "precedents" } else { "dependents" }
        show_method          = if ($Forward) { "ShowPrecedents" } else { "ShowDependents" }
        show_succeeded       = $false
        show_error           = $null
        direct_relationships = @()
        navigate_arrows      = @()
    }
    $worksheet = $null
    $cell = $null
    try {
        $worksheet = $Workbook.Worksheets.Item([string]$Seed.sheet)
        $cell = $worksheet.Range(([string]$Seed.address).Replace("$", ""))
        try {
            if ($Forward) { $cell.ShowPrecedents() } else { $cell.ShowDependents() }
            $result.show_succeeded = $true
        }
        catch { $result.show_error = $_.Exception.Message }

        $direct = $null
        try {
            if ($Forward) { $direct = $cell.DirectPrecedents } else { $direct = $cell.DirectDependents }
            if ($null -ne $direct) { $result.direct_relationships += Get-AddressFromRange $direct }
        }
        catch {
            $result.direct_error = $_.Exception.Message
        }
        Release-ComObject $direct

        for ($arrow = 1; $arrow -le 8; $arrow++) {
            $navigated = $null
            try {
                $navigated = $cell.NavigateArrow($Forward, $arrow, 1)
                if ($null -eq $navigated) { break }
                $target = Get-AddressFromRange $navigated
                $target.arrow_number = $arrow
                $target.exposed_by = "NavigateArrow"
                $result.navigate_arrows += $target
            }
            catch {
                if ($arrow -eq 1) { $result.navigate_error = $_.Exception.Message }
                break
            }
            finally { Release-ComObject $navigated }
        }
        try { $worksheet.ClearArrows() } catch {}
    }
    catch { $result.error = $_.Exception.Message }
    finally {
        Release-ComObject $cell
        Release-ComObject $worksheet
    }
    return $result
}

function Get-FormualizerEvidence {
    param([string]$BaselinePath, [string]$RuntimePath)
    $baseline = Get-Content -Raw $BaselinePath | ConvertFrom-Json
    $runtime = Get-Content -Raw $RuntimePath | ConvertFrom-Json
    $static = New-Object 'System.Collections.Generic.HashSet[string]'
    foreach ($address in $baseline.steps[0].main_passes[0].changed_member_addresses) {
        [void]$static.Add((Normalize-Address ([string]$address)))
    }
    $runtimeSamples = New-Object 'System.Collections.Generic.HashSet[string]'
    foreach ($address in $baseline.steps[0].main_runtime.volatile_member_samples) {
        [void]$runtimeSamples.Add((Normalize-Address ([string]$address)))
    }
    foreach ($address in $baseline.steps[0].main_runtime.dynamic_member_samples) {
        [void]$runtimeSamples.Add((Normalize-Address ([string]$address)))
    }
    return [ordered]@{
        source                                  = $BaselinePath
        static_scc_member_count                 = [int]$baseline.static_scc_probe.largest_scc_size
        runtime_live_member_count               = [int]$runtime.runtime_live_cycle_member_count
        static_membership_addresses             = @($static)
        runtime_live_full_address_set_available = $false
        runtime_live_membership_note            = "Existing verified artifact records the runtime-live count and samples, not the complete address set. Seed membership is unknown unless independently enumerated by a runtime artifact."
        runtime_live_known_sample_addresses     = @($runtimeSamples)
        main_scc_id                             = [uint64]$baseline.steps[0].main_scc_id
        main_runtime                            = $baseline.steps[0].main_runtime
        edge_origin_breakdown                   = Get-Content -Raw (Join-Path (Split-Path $RuntimePath) "fossil-static-edge-origin-breakdown.json") | ConvertFrom-Json
        runtime_topology                        = $runtime
    }
}

function Add-FormualizerMembership {
    param([object]$Seed, [object]$FzEvidence)
    $key = [string]$Seed.normalized_address
    $static = $FzEvidence.static_membership_addresses -contains $key
    $runtimeSample = $FzEvidence.runtime_live_known_sample_addresses -contains $key
    $Seed.formualizer_static_scc_member = $static
    $Seed.formualizer_runtime_live_membership = if ($runtimeSample) { "known_in_runtime_sample_only" } else { "unknown" }
    $Seed.formualizer_runtime_live_membership_proven = $false
    return $Seed
}

function Get-InterventionGroups {
    param([object]$FzEvidence)
    $parsed = @()
    foreach ($address in $FzEvidence.static_membership_addresses) {
        $text = [string]$address
        $bang = $text.LastIndexOf("!")
        if ($bang -lt 1) { continue }
        $sheet = $text.Substring(0, $bang)
        $cell = $text.Substring($bang + 1)
        $match = [regex]::Match($cell, "^(?:[A-Z]+)([0-9]+)$")
        if ($match.Success) {
            $parsed += [pscustomobject]@{ address = $text; sheet = $sheet; row = [int]$match.Groups[1].Value }
        }
    }
    $groups = @()
    foreach ($sheetGroup in ($parsed | Group-Object sheet)) {
        $minRow = ($sheetGroup.Group | Measure-Object row -Minimum).Minimum
        $maxRow = ($sheetGroup.Group | Measure-Object row -Maximum).Maximum
        $groups += [ordered]@{
            id           = "sheet:$($sheetGroup.Name)"
            strategy     = "sheet_used_range"
            sheet        = $sheetGroup.Name
            member_count = $sheetGroup.Count
        }
        $bandWidth = [math]::Max(1, [math]::Ceiling(($maxRow - $minRow + 1) / 4))
        for ($band = 0; $band -lt 4; $band++) {
            $low = $minRow + ($band * $bandWidth)
            $high = $low + $bandWidth - 1
            $members = @($sheetGroup.Group | Where-Object { $_.row -ge $low -and $_.row -le $high })
            if ($members.Count -gt 0) {
                $groups += [ordered]@{
                    id           = "sheet:$($sheetGroup.Name):row-band:$band"
                    strategy     = "row_band"
                    sheet        = $sheetGroup.Name
                    row_min      = $low
                    row_max      = $high
                    member_count = $members.Count
                }
            }
        }
    }
    return @($groups)
}

function Freeze-FormulaGroup {
    param([object]$Workbook, [object]$Group)
    $worksheet = $null
    $range = $null
    $errors = @()
    try {
        $worksheet = $Workbook.Worksheets.Item([string]$Group.sheet)
        if ([string]$Group.strategy -eq "sheet_used_range") {
            $range = $worksheet.UsedRange
        }
        else {
            $rowAddress = "{0}:{1}" -f [int]$Group.row_min, [int]$Group.row_max
            $range = $worksheet.Rows($rowAddress)
        }
        try {
            $range.Copy()
            $range.PasteSpecial($xlPasteValues)
            return [ordered]@{ attempted = [int]$Group.member_count; succeeded = [int]$Group.member_count; errors = @(); bulk_range = [string]$range.Address($false, $false) }
        }
        catch {
            $errors += [ordered]@{ range = [string]$range.Address($false, $false); error = $_.Exception.Message }
            return [ordered]@{ attempted = [int]$Group.member_count; succeeded = 0; errors = $errors; bulk_range = [string]$range.Address($false, $false) }
        }
    }
    finally {
        Release-ComObject $range
        Release-ComObject $worksheet
    }
}

function Invoke-Intervention {
    param([object]$Group, [string]$SourcePath, [string]$TempRoot, [object]$FzEvidence)
    $copy = Join-Path $TempRoot (([guid]::NewGuid().ToString()) + ".xlsx")
    $session = $null
    try {
        $session = New-OracleSession -SourcePath $SourcePath -CopyPath $copy -Iteration $true -MaxIterations 100 -MaxChange 0.001 -MultiThreaded $true
        $beforeRebuild = Invoke-FullRebuild $session.excel
        $beforeSeeds = @(Get-CircularSeeds $session.workbook)
        $freeze = Freeze-FormulaGroup -Workbook $session.workbook -Group $Group
        $afterRebuild = Invoke-FullRebuild $session.excel
        $afterSeeds = @(Get-CircularSeeds $session.workbook)
        foreach ($seed in $beforeSeeds) { Add-FormualizerMembership -Seed $seed -FzEvidence $FzEvidence }
        foreach ($seed in $afterSeeds) { Add-FormualizerMembership -Seed $seed -FzEvidence $FzEvidence }
        return [ordered]@{
            group          = $Group
            before_rebuild = $beforeRebuild
            before_seeds   = $beforeSeeds
            freeze         = $freeze
            after_rebuild  = $afterRebuild
            after_seeds    = $afterSeeds
            interpretation = "Excel-observable feedback intervention only; not full SCC membership."
        }
    }
    finally {
        Close-OracleSession $session
        if (Test-Path $copy) { Remove-Item $copy -Force }
    }
}

function Invoke-DependencyTracePhase {
    param([string]$SourcePath, [string]$TempRoot, [object[]]$Seeds)
    $copy = Join-Path $TempRoot (([guid]::NewGuid().ToString()) + ".xlsx")
    $session = $null
    try {
        $session = New-OracleSession -SourcePath $SourcePath -CopyPath $copy -Iteration $true -MaxIterations 100 -MaxChange 0.001 -MultiThreaded $true
        $rebuild = Invoke-FullRebuild $session.excel
        $paths = @()
        foreach ($seed in $Seeds) {
            $paths += Trace-DependencyDirection -Excel $session.excel -Workbook $session.workbook -Seed $seed -Forward $true
            $paths += Trace-DependencyDirection -Excel $session.excel -Workbook $session.workbook -Seed $seed -Forward $false
        }
        return [ordered]@{ rebuild = $rebuild; paths = $paths }
    }
    finally {
        Close-OracleSession $session
        if (Test-Path $copy) { Remove-Item $copy -Force }
    }
}

function Invoke-SeedPhase {
    param([string]$SourcePath, [string]$TempRoot, [bool]$Iteration, [object]$FzEvidence)
    $copy = Join-Path $TempRoot (([guid]::NewGuid().ToString()) + ".xlsx")
    $session = $null
    try {
        $session = New-OracleSession -SourcePath $SourcePath -CopyPath $copy -Iteration $Iteration -MaxIterations 100 -MaxChange 0.001 -MultiThreaded $true
        $rebuild = Invoke-FullRebuild $session.excel
        $seeds = @(Get-CircularSeeds $session.workbook)
        foreach ($seed in $seeds) { Add-FormualizerMembership -Seed $seed -FzEvidence $FzEvidence }
        return [ordered]@{
            iteration_enabled    = $Iteration
            settings_before_open = $session.settings_before_open
            settings_after_open  = $session.settings_after_open
            settings_configured  = $session.settings_configured
            rebuild              = $rebuild
            seeds                = $seeds
        }
    }
    finally {
        Close-OracleSession $session
        if (Test-Path $copy) { Remove-Item $copy -Force }
    }
}

function Invoke-TimingCase {
    param([string]$SourcePath, [string]$TempRoot, [bool]$Iteration, [bool]$MultiThreaded, [int]$WarmRuns)
    $copy = Join-Path $TempRoot (([guid]::NewGuid().ToString()) + ".xlsx")
    $session = $null
    try {
        $session = New-OracleSession -SourcePath $SourcePath -CopyPath $copy -Iteration $Iteration -MaxIterations 100 -MaxChange 0.001 -MultiThreaded $MultiThreaded
        $baseline = Invoke-FullRebuild $session.excel
        $warm = @()
        for ($run = 1; $run -le $WarmRuns; $run++) {
            $watch = [System.Diagnostics.Stopwatch]::StartNew()
            $errorText = $null
            try { $session.excel.Calculate() | Out-Null; $state = Wait-ExcelCalculation $session.excel } catch { $errorText = $_.Exception.Message; $state = $null }
            $watch.Stop()
            $warm += [ordered]@{ run = $run; elapsed_ms = [math]::Round($watch.Elapsed.TotalMilliseconds, 3); calculation_state = $state; error = $errorText }
        }
        $full = @()
        foreach ($method in @("CalculateFull", "CalculateFullRebuild")) {
            $watch = [System.Diagnostics.Stopwatch]::StartNew()
            $errorText = $null
            try { $session.excel.$method(); $state = Wait-ExcelCalculation $session.excel } catch { $errorText = $_.Exception.Message; $state = $null }
            $watch.Stop()
            $full += [ordered]@{ method = $method; elapsed_ms = [math]::Round($watch.Elapsed.TotalMilliseconds, 3); calculation_state = $state; error = $errorText }
        }
        return [ordered]@{
            iteration_enabled      = $Iteration
            multi_threaded_enabled = $MultiThreaded
            settings_before_open   = $session.settings_before_open
            settings_after_open    = $session.settings_after_open
            settings_configured    = $session.settings_configured
            baseline_full_rebuild  = $baseline
            warm_calculate         = $warm
            full_methods           = $full
        }
    }
    finally {
        Close-OracleSession $session
        if (Test-Path $copy) { Remove-Item $copy -Force }
    }
}

function Get-CalcChainEvidence {
    param([string]$Path)
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [System.IO.Compression.ZipFile]::OpenRead($Path)
    try {
        $entry = $archive.GetEntry("xl/calcChain.xml")
        if ($null -eq $entry) { return [ordered]@{ present = $false; note = "No calcChain.xml entry." } }
        $reader = New-Object System.IO.StreamReader($entry.Open())
        try { $xml = $reader.ReadToEnd() } finally { $reader.Dispose() }
        $count = ([regex]::Matches($xml, "<c\b")).Count
        return [ordered]@{ present = $true; entry = "xl/calcChain.xml"; cell_record_count = $count; note = "Secondary calculation-order evidence only; not a dependency graph or edge proof." }
    }
    finally { $archive.Dispose() }
}

$source = (Resolve-Path $WorkbookPath).Path
$baseline = (Resolve-Path $FormualizerBaselinePath).Path
$runtime = (Resolve-Path $FormualizerRuntimePath).Path
$fzEvidence = Get-FormualizerEvidence -BaselinePath $baseline -RuntimePath $runtime
$tempRoot = Join-Path $env:TEMP ("formualizer-excel-circular-" + $PID)
New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null
$excel = $null
$originalSettings = $null
try {
    $excel = New-Object -ComObject Excel.Application
    $excel.Visible = $false
    $excel.DisplayAlerts = $false
    try { $excel.AutomationSecurity = 3 } catch {}
    $originalSettings = Get-ExcelSettings $excel
    try { $excel.Quit() } catch {}
    Release-ComObject $excel
    $excel = $null

    $seedEnabled = Invoke-SeedPhase -SourcePath $source -TempRoot $tempRoot -Iteration $true -FzEvidence $fzEvidence
    $seedDisabled = Invoke-SeedPhase -SourcePath $source -TempRoot $tempRoot -Iteration $false -FzEvidence $fzEvidence
    $allSeeds = @($seedEnabled.seeds + $seedDisabled.seeds)
    $uniqueSeeds = @($allSeeds | Group-Object normalized_address | ForEach-Object { $_.Group[0] })
    $trace = Invoke-DependencyTracePhase -SourcePath $source -TempRoot $tempRoot -Seeds $uniqueSeeds

    $groups = Get-InterventionGroups $fzEvidence
    $interventions = @()
    foreach ($group in $groups) {
        $interventions += Invoke-Intervention -Group $group -SourcePath $source -TempRoot $tempRoot -FzEvidence $fzEvidence
    }

    $timings = @()
    foreach ($iteration in @($true, $false)) {
        foreach ($multiThreaded in @($true, $false)) {
            $timings += Invoke-TimingCase -SourcePath $source -TempRoot $tempRoot -Iteration $iteration -MultiThreaded $multiThreaded -WarmRuns $TimingWarmRuns
        }
    }

    $result = [ordered]@{
        schema                      = "formualizer.excel-circular-set-oracle/v1"
        generated_at_utc            = [DateTime]::UtcNow.ToString("o")
        workbook                    = $source
        original_excel_settings     = $originalSettings
        phase1_circular_seeds       = [ordered]@{ iteration_enabled = $seedEnabled; iteration_disabled = $seedDisabled }
        unique_seed_count           = $uniqueSeeds.Count
        phase2_dependency_trace     = $trace
        phase3_interventions        = $interventions
        phase5_calculation_behavior = $timings
        phase6_calc_chain           = Get-CalcChainEvidence $source
        formualizer_evidence        = $fzEvidence
        notes                       = @(
            "Worksheet.CircularReference exposes at most the first circular reference on a worksheet; it is not full circular-set enumeration.",
            "Excel dependency tracing is recorded only when exposed by DirectPrecedents/DirectDependents, ShowPrecedents/ShowDependents, or NavigateArrow.",
            "Interventions use fresh disposable workbook copies and identify Excel-observable feedback/cut structure only, not full SCC membership.",
            "Runtime-live membership is marked unknown unless supplied by an explicit runtime address artifact; static membership is not substituted."
        )
    }
    $json = $result | ConvertTo-Json -Depth 40
    [System.IO.File]::WriteAllText($OutputPath, $json + [Environment]::NewLine, [System.Text.UTF8Encoding]::new($false))
    Write-Output "Generated $OutputPath"
}
finally {
    if ($null -ne $excel) { try { $excel.Quit() } catch {}; Release-ComObject $excel }
    if (Test-Path $tempRoot) { Remove-Item $tempRoot -Recurse -Force }
}
