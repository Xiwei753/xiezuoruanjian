@if "%DEBUG%"=="" @echo off
@rem ##########################################################################
@rem
@rem  Hvigor startup script for Windows
@rem
@rem ##########################################################################

@rem Set local scope for the variables with windows NT shell
if "%OS%"=="Windows_NT" setlocal

set DIRNAME=%~dp0
if "%DIRNAME%"=="" set DIRNAME=.
@rem This is normally unused
set APP_BASE_NAME=%~n0
set APP_HOME=%DIRNAME%

@rem Resolve any "." and ".." in APP_HOME to make it shorter.
for %%i in ("%APP_HOME%") do set APP_HOME=%%~fi

@rem Find node.exe
if defined NODE_HOME goto findNodeFromNodeHome

set NODE_EXE=node.exe
%NODE_EXE% --version >NUL 2>&1
if %ERRORLEVEL% equ 0 goto execute

echo.
echo ERROR: NODE_HOME is not set and no 'node' command could be found in your PATH.
echo.
echo Please set the NODE_HOME variable in your environment to match the
echo location of your Node installation.

goto fail

:findNodeFromNodeHome
set NODE_HOME=%NODE_HOME:"=%
set NODE_EXE=%NODE_HOME%/node.exe

if exist "%NODE_EXE%" goto execute

echo.
echo ERROR: NODE_HOME is set to an invalid directory: %NODE_HOME%
echo.
echo Please set the NODE_HOME variable in your environment to match the
echo location of your Node installation.

goto fail

:execute
@rem Setup the command line

set HVIGOR_WRAPPER_JS=%APP_HOME%\hvigor\hvigor-wrapper.js

if not exist "%HVIGOR_WRAPPER_JS%" (
    echo Warning: hvigor-wrapper.js not found at %HVIGOR_WRAPPER_JS%
    echo Attempting ohpm install to fetch dependencies...
    cd /d "%APP_HOME%"
    ohpm install --all 2>NUL
)

if not exist "%HVIGOR_WRAPPER_JS%" (
    echo ERROR: hvigor-wrapper.js still not found after ohpm install.
    echo Please ensure @ohos/hvigor-ohos is in your dependencies.
    goto fail
)

@rem Execute hvigor
"%NODE_EXE%" --expose-gc "%HVIGOR_WRAPPER_JS%" %*

:end
@rem End local scope for the variables with windows NT shell
if %OS%==Windows_NT endlocal

:omega
exit /b %ERRORLEVEL%

:fail
rem Set variable HVIGOR_EXIT_CONSOLE if you need the _script_ return code instead of
rem the _cmd.exe /c_ return code!
set EXIT_CODE=%ERRORLEVEL%
if %EXIT_CODE% equ 0 set EXIT_CODE=1
if not ""=="%HVIGOR_EXIT_CONSOLE%" exit %EXIT_CODE%
exit /b %EXIT_CODE%