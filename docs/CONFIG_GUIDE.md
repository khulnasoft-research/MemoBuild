# CONFIG_GUIDE.md for MemoBuild

# MemoBuild Configuration Guide

This document provides a comprehensive guide to the `memobuild.yaml` configuration file used in the MemoBuild project. Below you will find all available options, along with explanations and examples.

## Table of Contents

1. [Overview](#overview)
2. [Configuration Options](#configuration-options)
   - [Global Options](#global-options)
   - [Library Options](#library-options)
   - [Service Options](#service-options)
3. [Examples](#examples)
4. [Additional Resources](#additional-resources)

## Overview

The `memobuild.yaml` configuration file defines the settings and parameters necessary for running MemoBuild. It includes options for setting global parameters, libraries, and services used in the project.

## Configuration Options

### Global Options

```yaml
# Global settings for the project
version: '1.0'
encoding: 'utf-8'
log_level: 'info'
```

- `version`: Specifies the version of your configuration.
- `encoding`: Sets the character encoding used.
- `log_level`: Defines the logging level (e.g., debug, info, warning, error).

### Library Options

```yaml
# Library settings for dependencies
libraries:
  - name: 'example-library'
    version: '1.2.3'
    path: 'path/to/example-library'
```

- `name`: The name of the library to include.
- `version`: The version of the library to use.
- `path`: The path to the library's codebase.

### Service Options

```yaml
# Settings for services
services:
  - name: 'example-service'
    type: 'http'
    endpoint: 'http://example.com/api'
    timeout: 30
```

- `name`: The name of the service.
- `type`: The type of service (e.g., http, grpc).
- `endpoint`: The URL endpoint for the service.
- `timeout`: Timeout value in seconds for service requests.

## Examples

### Basic Configuration

```yaml
version: '1.0'
encoding: 'utf-8'
log_level: 'info'
libraries:
  - name: 'example-library'
    version: '1.2.3'
    path: 'libs/example-library'
services:
  - name: 'example-service'
    type: 'http'
    endpoint: 'http://example.com/api'
    timeout: 30
```

### Advanced Configuration

```yaml
version: '1.1'
encoding: 'utf-8'
log_level: 'debug'
libraries:
  - name: 'my-library'
    version: '2.0.0'
    path: 'libs/my-library'
  - name: 'another-library'
    version: '1.0.0'
    path: 'libs/another-library'
services:
  - name: 'my-service'
    type: 'http'
    endpoint: 'http://my-service.com/api'
    timeout: 20
  - name: 'grpc-service'
    type: 'grpc'
    endpoint: 'grpc://my-grpc-service:50051'
    timeout: 40
```

## Additional Resources

- [MemoBuild Documentation](https://example.com/docs)
- [GitHub Repository](https://github.com/nrelab/MemoBuild)

---

This concludes the configuration guide for `memobuild.yaml`. Please refer to the documentation and GitHub repository for further information and updates.