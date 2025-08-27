#!/usr/bin/env python3
"""
Validation script to compare YAML methodology files with JSON schema files.
Checks if the object structures described in both files are consistent.
"""

import json
import yaml
import sys
from pathlib import Path
from typing import Dict, List, Set, Tuple, Any


class SchemaMethodologyValidator:
    def __init__(self, schema_dir: Path, methodology_dir: Path):
        self.schema_dir = schema_dir
        self.methodology_dir = methodology_dir
        self.errors = []
        self.warnings = []
        
    def load_yaml(self, file_path: Path) -> Dict:
        """Load YAML file"""
        with open(file_path, 'r', encoding='utf-8') as f:
            return yaml.safe_load(f)
    
    def load_json(self, file_path: Path) -> Dict:
        """Load JSON schema file"""
        with open(file_path, 'r', encoding='utf-8') as f:
            return json.load(f)
    
    def extract_yaml_paths(self, data: Any, current_path: str = "") -> Set[str]:
        """
        Extract all object paths from YAML methodology file.
        Ignores <rules> elements as they are additions.
        """
        paths = set()
        
        if isinstance(data, dict):
            for key, value in data.items():
                if key == '<rules>':
                    # Skip rules as they are methodology additions
                    continue
                elif key in ['metadata', 'global_rules']:
                    # Skip metadata and global rules sections
                    continue
                    
                new_path = f"{current_path}.{key}" if current_path else key
                paths.add(new_path)
                
                # Recursively extract paths
                sub_paths = self.extract_yaml_paths(value, new_path)
                paths.update(sub_paths)
        
        return paths
    
    def extract_schema_paths(self, schema: Dict, current_path: str = "") -> Set[str]:
        """Extract all object paths from JSON schema"""
        paths = set()
        
        if not isinstance(schema, dict):
            return paths
        
        # Handle array types - if it's an array with items, extract from items
        if schema.get('type') == 'array' and 'items' in schema:
            items_schema = schema['items']
            # For arrays, we treat the items as if they were direct properties
            # This matches how YAML paths are structured (e.g., exchanges.exchange.meanAmount)
            if isinstance(items_schema, dict):
                # If items is an object or has properties, extract those paths
                if items_schema.get('type') == 'object' or 'properties' in items_schema:
                    sub_paths = self.extract_schema_paths(items_schema, current_path)
                    paths.update(sub_paths)
                else:
                    # For simple array items, add the current path
                    paths.add(current_path)
            elif isinstance(items_schema, list):
                # Handle tuple validation (array of different schemas)
                for item in items_schema:
                    sub_paths = self.extract_schema_paths(item, current_path)
                    paths.update(sub_paths)
        
        # Handle properties in JSON schema
        if 'properties' in schema:
            for prop_name, prop_schema in schema['properties'].items():
                # Handle namespaced properties (e.g., common:UUID)
                clean_name = prop_name.replace('common:', '').replace('@', '')
                new_path = f"{current_path}.{clean_name}" if current_path else clean_name
                paths.add(new_path)
                
                # Recursively extract paths
                sub_paths = self.extract_schema_paths(prop_schema, new_path)
                paths.update(sub_paths)
        
        # Handle nested type objects without properties
        elif 'type' in schema and schema['type'] == 'object':
            if 'properties' in schema:
                return self.extract_schema_paths({'properties': schema['properties']}, current_path)
        
        return paths
    
    def normalize_path(self, path: str) -> str:
        """Normalize path for comparison"""
        # Remove namespace prefixes
        path = path.replace('common:', '')
        path = path.replace('@', '')
        
        # Handle special cases
        replacements = {
            'UUID': 'uuid',
            'timeStamp': 'timestamp',
            'dataSetVersion': 'datasetversion',
        }
        
        for old, new in replacements.items():
            path = path.replace(old, new)
        
        return path.lower()
    
    def compare_structures(self, yaml_file: Path, schema_file: Path) -> Tuple[List[str], List[str]]:
        """Compare YAML methodology structure with JSON schema structure"""
        errors = []
        warnings = []
        
        try:
            # Load files
            yaml_data = self.load_yaml(yaml_file)
            schema_data = self.load_json(schema_file)
            
            # Extract paths
            yaml_paths = self.extract_yaml_paths(yaml_data)
            schema_paths = self.extract_schema_paths(schema_data)
            
            # Normalize paths for comparison
            yaml_paths_normalized = {self.normalize_path(p): p for p in yaml_paths}
            schema_paths_normalized = {self.normalize_path(p): p for p in schema_paths}
            
            # Find mismatches
            yaml_only = set(yaml_paths_normalized.keys()) - set(schema_paths_normalized.keys())
            schema_only = set(schema_paths_normalized.keys()) - set(yaml_paths_normalized.keys())
            
            # Report fields in YAML but not in schema
            for norm_path in yaml_only:
                orig_path = yaml_paths_normalized[norm_path]
                # Skip certain expected differences
                if any(skip in orig_path for skip in ['metadata', 'global_rules']):
                    continue
                warnings.append(f"Field '{orig_path}' in YAML methodology not found in schema")
            
            # Report important fields in schema but not in YAML
            important_schema_fields = [
                'processDataSet',
                'processInformation',
                'modellingAndValidation',
                'administrativeInformation',
                'exchanges'
            ]
            
            for norm_path in schema_only:
                orig_path = schema_paths_normalized[norm_path]
                # Only report top-level important fields
                if any(field in orig_path for field in important_schema_fields):
                    top_level = orig_path.split('.')[0]
                    if top_level in important_schema_fields:
                        warnings.append(f"Schema field '{orig_path}' not covered in YAML methodology")
            
        except Exception as e:
            errors.append(f"Error processing files: {str(e)}")
        
        return errors, warnings
    
    def validate_all(self) -> Dict[str, Dict]:
        """Validate all YAML files against their corresponding schemas"""
        results = {}
        
        # Find all YAML methodology files
        yaml_files = list(self.methodology_dir.glob("*.yaml"))
        
        for yaml_file in yaml_files:
            # Find corresponding schema file
            base_name = yaml_file.stem
            schema_file = self.schema_dir / f"{base_name}.json"
            
            if not schema_file.exists():
                results[yaml_file.name] = {
                    'status': 'error',
                    'errors': [f"No corresponding schema file found: {schema_file.name}"],
                    'warnings': []
                }
                continue
            
            # Compare structures
            errors, warnings = self.compare_structures(yaml_file, schema_file)
            
            results[yaml_file.name] = {
                'status': 'error' if errors else ('warning' if warnings else 'ok'),
                'errors': errors,
                'warnings': warnings,
                'schema_file': schema_file.name
            }
        
        return results
    
    def print_report(self, results: Dict[str, Dict]):
        """Print validation report"""
        print("=" * 80)
        print("YAML Methodology vs JSON Schema Validation Report")
        print("=" * 80)
        print()
        
        total_files = len(results)
        error_count = sum(1 for r in results.values() if r['status'] == 'error')
        warning_count = sum(1 for r in results.values() if r['status'] == 'warning')
        ok_count = sum(1 for r in results.values() if r['status'] == 'ok')
        
        for yaml_file, result in results.items():
            print(f"File: {yaml_file}")
            print(f"Schema: {result.get('schema_file', 'N/A')}")
            print(f"Status: {result['status'].upper()}")
            
            if result['errors']:
                print("\nERRORS:")
                for error in result['errors']:
                    print(f"  ❌ {error}")
            
            if result['warnings']:
                print("\nWARNINGS:")
                for warning in result['warnings']:
                    print(f"  ⚠️  {warning}")
            
            if result['status'] == 'ok':
                print("  ✅ All fields validated successfully")
            
            print("-" * 80)
        
        print("\nSUMMARY:")
        print(f"  Total files checked: {total_files}")
        print(f"  ✅ Passed: {ok_count}")
        print(f"  ⚠️  Warnings: {warning_count}")
        print(f"  ❌ Errors: {error_count}")
        
        return error_count == 0


def main():
    """Main entry point"""
    # Set up paths
    base_dir = Path(__file__).parent
    schema_dir = base_dir / "tidas" / "schemas"
    methodology_dir = base_dir / "tidas" / "methodologies"
    
    # Validate directories exist
    if not schema_dir.exists():
        print(f"Error: Schema directory not found: {schema_dir}")
        sys.exit(1)
    
    if not methodology_dir.exists():
        print(f"Error: Methodology directory not found: {methodology_dir}")
        sys.exit(1)
    
    # Run validation
    validator = SchemaMethodologyValidator(schema_dir, methodology_dir)
    results = validator.validate_all()
    
    # Print report
    success = validator.print_report(results)
    
    # Exit with appropriate code
    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()