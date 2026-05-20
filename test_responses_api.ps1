# PowerShell Script to Test OpenAI Responses API
# Version: 2.0
# Description: Complete test script for /v1/responses endpoint

param(
    [string]$BaseUrl = "http://localhost:22217",
    [string]$ApiKey = "",
    [switch]$Stream,
    [switch]$All,
    [switch]$Help
)

$ENDPOINT = "/v1/responses"

function Write-Banner {
    Write-Host ""
    Write-Host "==================================================" -ForegroundColor Cyan
    Write-Host "  OpenAI Responses API Test Tool v2.0" -ForegroundColor Cyan
    Write-Host "==================================================" -ForegroundColor Cyan
    Write-Host ""
}

function Write-Info {
    param([string]$Message)
    Write-Host "[INFO] $Message" -ForegroundColor Yellow
}

function Write-Success {
    param([string]$Message)
    Write-Host "[SUCCESS] $Message" -ForegroundColor Green
}

function Write-Error {
    param([string]$Message)
    Write-Host "[ERROR] $Message" -ForegroundColor Red
}

function Show-Help {
    Write-Banner
    Write-Host "Usage:" -ForegroundColor White
    Write-Host "  .\test_responses_api.ps1 -ApiKey `"sk-your-key`" [options]" -ForegroundColor White
    Write-Host ""
    Write-Host "Parameters:" -ForegroundColor Cyan
    Write-Host "  -BaseUrl  : API server URL (default: http://localhost:22217)" -ForegroundColor White
    Write-Host "  -ApiKey   : Your API key (required)" -ForegroundColor White
    Write-Host "  -Stream   : Test streaming mode" -ForegroundColor White
    Write-Host "  -All      : Test both streaming and non-streaming" -ForegroundColor White
    Write-Host "  -Help     : Show this help message" -ForegroundColor White
    Write-Host ""
    Write-Host "Examples:" -ForegroundColor Cyan
    Write-Host '  .\test_responses_api.ps1 -ApiKey "sk-1234567890"' -ForegroundColor White
    Write-Host '  .\test_responses_api.ps1 -ApiKey "sk-1234567890" -Stream' -ForegroundColor White
    Write-Host '  .\test_responses_api.ps1 -ApiKey "sk-1234567890" -All' -ForegroundColor White
    Write-Host '  .\test_responses_api.ps1 -BaseUrl "http://localhost:8080" -ApiKey "sk-1234567890" -All' -ForegroundColor White
    Write-Host ""
}

function Test-NonStream {
    Write-Host ""
    Write-Host "==================================================" -ForegroundColor Cyan
    Write-Host "  Test 1: Non-Streaming Response" -ForegroundColor Cyan
    Write-Host "==================================================" -ForegroundColor Cyan
    Write-Host ""

    $body = @{
        model = "deepseek-chat"
        input = "What is 2 + 2?"
        stream = $false
    } | ConvertTo-Json -Depth 10

    $headers = @{
        "Authorization" = "Bearer $ApiKey"
        "Content-Type" = "application/json"
    }

    Write-Info "Sending request to $BaseUrl$ENDPOINT"
    Write-Host ""
    Write-Host "Request Body:" -ForegroundColor White
    Write-Host $body -ForegroundColor Gray
    Write-Host ""

    try {
        $response = Invoke-WebRequest -Uri "$BaseUrl$ENDPOINT" `
            -Method POST `
            -Headers $headers `
            -Body $body `
            -TimeoutSec 60

        Write-Host ""
        Write-Host "Response Status: $($response.StatusCode) $($response.StatusDescription)" -ForegroundColor Green

        if ($response.Content) {
            $json = $response.Content | ConvertFrom-Json

            Write-Host ""
            Write-Host "==================================================" -ForegroundColor Cyan
            Write-Host "  Response Structure Validation" -ForegroundColor Cyan
            Write-Host "==================================================" -ForegroundColor Cyan
            Write-Host ""

            $fields = @{
                "id" = $json.id
                "object" = $json.object
                "status" = $json.status
                "model" = $json.model
                "created_at" = $json.created_at
            }

            $optionalFields = @{
                "service_tier" = $json.service_tier
                "output" = $json.output
                "usage" = $json.usage
                "metadata" = $json.metadata
            }

            Write-Host "Required Fields:" -ForegroundColor White
            foreach ($field in $fields.GetEnumerator()) {
                if ($field.Value -ne $null) {
                    Write-Host "  [OK] $($field.Key) = $($field.Value)" -ForegroundColor Green
                } else {
                    Write-Host "  [MISSING] $($field.Key)" -ForegroundColor Red
                }
            }

            Write-Host ""
            Write-Host "Optional Fields:" -ForegroundColor White
            foreach ($field in $optionalFields.GetEnumerator()) {
                if ($field.Value -ne $null) {
                    Write-Host "  [OK] $($field.Key) = $($field.Value)" -ForegroundColor Green
                } else {
                    Write-Host "  [SKIP] $($field.Key) = null" -ForegroundColor Gray
                }
            }

            Write-Host ""
            Write-Host "==================================================" -ForegroundColor Cyan
            Write-Host "  Full Response JSON" -ForegroundColor Cyan
            Write-Host "==================================================" -ForegroundColor Cyan
            Write-Host ""
            $json | ConvertTo-Json -Depth 10 | Write-Host -ForegroundColor White
        }

        return $true
    } catch {
        Write-Host ""
        Write-Error "Request failed: $($_.Exception.Message)"
        if ($_.Exception.Response) {
            Write-Error "Status Code: $($_.Exception.Response.StatusCode)"
            try {
                $errorBody = [System.IO.StreamReader]::new($_.Exception.Response.GetResponseStream()).ReadToEnd()
                Write-Host "Response Body: $errorBody" -ForegroundColor Red
            } catch {
            }
        }
        return $false
    }
}

function Test-Stream {
    Write-Host ""
    Write-Host "==================================================" -ForegroundColor Cyan
    Write-Host "  Test 2: Streaming Response (SSE)" -ForegroundColor Cyan
    Write-Host "==================================================" -ForegroundColor Cyan
    Write-Host ""

    $body = @{
        model = "deepseek-chat"
        input = "What is 2 + 2?"
        stream = $true
    } | ConvertTo-Json -Depth 10

    $headers = @{
        "Authorization" = "Bearer $ApiKey"
        "Content-Type" = "application/json"
    }

    Write-Info "Sending streaming request to $BaseUrl$ENDPOINT"
    Write-Host ""
    Write-Host "Request Body:" -ForegroundColor White
    Write-Host $body -ForegroundColor Gray
    Write-Host ""

    try {
        $response = Invoke-WebRequest -Uri "$BaseUrl$ENDPOINT" `
            -Method POST `
            -Headers $headers `
            -Body $body `
            -TimeoutSec 60 `
            -Stream

        Write-Host ""
        Write-Host "Response Status: $($response.StatusCode) $($response.StatusDescription)" -ForegroundColor Green

        Write-Host ""
        Write-Host "==================================================" -ForegroundColor Cyan
        Write-Host "  SSE Events (first 10)" -ForegroundColor Cyan
        Write-Host "==================================================" -ForegroundColor Cyan
        Write-Host ""

        $eventCount = 0
        $totalLines = 0

        $response.Content -split "`n" | ForEach-Object {
            $totalLines++
            if ($_ -match '^data: (.+)') {
                $eventCount++
                $eventData = $Matches[1]

                if ($eventData -eq "[DONE]") {
                    Write-Host "data: [DONE]" -ForegroundColor Green
                } else {
                    Write-Host "data: $eventData" -ForegroundColor White
                }

                if ($eventCount -ge 10) {
                    Write-Host ""
                    Write-Host "... (stream continues, showing first 10 events)" -ForegroundColor Yellow
                    return
                }
            }
        }

        Write-Host ""
        Write-Info "Total lines received: $totalLines"
        Write-Info "Total events received: $eventCount"

        return $true
    } catch {
        Write-Host ""
        Write-Error "Stream request failed: $($_.Exception.Message)"
        if ($_.Exception.Response) {
            Write-Error "Status Code: $($_.Exception.Response.StatusCode)"
        }
        return $false
    }
}

function Main {
    if ($Help) {
        Show-Help
        return
    }

    Write-Banner

    Write-Host "Configuration:" -ForegroundColor Cyan
    Write-Host "  Base URL : $BaseUrl" -ForegroundColor White
    Write-Host "  Endpoint : $ENDPOINT" -ForegroundColor White
    Write-Host "  API Key  : $($ApiKey.Substring(0, [Math]::Min(10, $ApiKey.Length)))..." -ForegroundColor White
    Write-Host ""

    if ([string]::IsNullOrEmpty($ApiKey)) {
        Write-Error "API Key is required!"
        Write-Host ""
        Show-Help
        exit 1
    }

    $results = @{
        NonStream = $false
        Stream = $false
    }

    if ($All) {
        $results.NonStream = Test-NonStream
        $results.Stream = Test-Stream
    } elseif ($Stream) {
        $results.Stream = Test-Stream
    } else {
        $results.NonStream = Test-NonStream
    }

    Write-Host ""
    Write-Host "==================================================" -ForegroundColor Cyan
    Write-Host "  Test Summary" -ForegroundColor Cyan
    Write-Host "==================================================" -ForegroundColor Cyan
    Write-Host ""

    if ($All -or (-not $Stream)) {
        if ($results.NonStream) {
            Write-Success "Non-Streaming Test: PASSED"
        } else {
            Write-Error "Non-Streaming Test: FAILED"
        }
    }

    if ($All -or $Stream) {
        if ($results.Stream) {
            Write-Success "Streaming Test: PASSED"
        } else {
            Write-Error "Streaming Test: FAILED"
        }
    }

    Write-Host ""
    Write-Host "==================================================" -ForegroundColor Cyan
    Write-Host ""

    $allPassed = ($results.NonStream -or -not $All -or -not $Stream) -and ($results.Stream -or $All -or -not $Stream)
    if ($allPassed) {
        exit 0
    } else {
        exit 1
    }
}

Main
