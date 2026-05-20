"""Error types for external LCA imports."""


class ImportLcaError(Exception):
    """Base error for the external LCA import layer."""

    issue_code = "import_lca_error"


class UnsupportedFormatError(ImportLcaError):
    """Raised when a source format is known but intentionally unsupported."""

    issue_code = "unsupported_format"


class AmbiguousFormatError(ImportLcaError):
    """Raised when automatic source format detection finds multiple candidates."""

    issue_code = "ambiguous_format"


class AdapterNotImplementedError(ImportLcaError):
    """Raised when a source adapter has not been implemented yet."""

    issue_code = "adapter_not_implemented"
