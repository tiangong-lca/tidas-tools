import logging
import sys


class ColoredFormatter(logging.Formatter):
    # Define colors corresponding to different log levels
    COLORS = {
        logging.DEBUG: "\033[36m",  # Cyan
        logging.INFO: "\033[32m",  # Green
        logging.WARNING: "\033[33m",  # Yellow
        logging.ERROR: "\033[31m",  # Red
        logging.CRITICAL: "\033[41m",  # Red background
    }
    RESET = "\033[0m"

    def format(self, record):
        # First, get the original log message
        message = super().format(record)
        # Choose the color based on the log level
        color = self.COLORS.get(record.levelno, self.RESET)
        return f"{color}{message}{self.RESET}"


def setup_logging(verbose, function_name):
    log_level = logging.DEBUG if verbose else logging.INFO

    # File Handler: no color formatting needed
    file_handler = logging.FileHandler(f"tidas_{function_name}.log", mode="w")
    file_formatter = logging.Formatter("%(asctime)s:%(message)s")
    file_handler.setFormatter(file_formatter)

    # Console Handler: use custom ColoredFormatter for colored output
    console_handler = logging.StreamHandler(sys.stdout)
    console_formatter = ColoredFormatter("%(asctime)s:%(message)s")
    console_handler.setFormatter(console_formatter)

    # Configure the root logger with the file and console handlers
    logging.basicConfig(
        level=log_level,
        handlers=[file_handler, console_handler],
    )
