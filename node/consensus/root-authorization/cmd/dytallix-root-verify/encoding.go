package main

import "encoding/json"

func jsonBytes(value any) ([]byte, error) { return json.Marshal(value) }
